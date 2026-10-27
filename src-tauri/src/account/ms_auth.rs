// 微软正版认证：设备码登录 + XBL/XSTS/Minecraft 认证链 + token 刷新
//
// 认证链（wiki.vg Microsoft Authentication）：
//   1. login.microsoftonline.com 设备码流程 → MS access_token + refresh_token
//      scope 必须含 XboxLive.signin（进 Xbox 体系）与 offline_access（拿刷新令牌）
//   2. user.auth.xboxlive.com → XBL token + uhs
//   3. xsts.auth.xboxlive.com → XSTS token + uhs（XErr 错误码映射为中文提示）
//   4. api.minecraftservices.com /authentication/login_with_xbox → MC access_token
//   5. /minecraft/profile → uuid / name / skins / capes（存入账户缓存）
//
// MC token 约 24h 有效：启动前经 ensure_mc_auth 校验并按需静默刷新。
// token 明文存 accounts.json（用户决策），不经 IPC 暴露给前端。
// 设备码流程状态经 ms-login-state 事件推送前端（code / pending / success /
// expired / denied / failed），同一时间仅允许一个流程（原子标志互斥）。
// 所有请求 reqwest 直连（项目约定：不跟随系统代理）。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tauri::Emitter;

use super::store::AccountStore;
use super::{Account, CapeEntry, SkinEntry};
use crate::launch::args::AuthContext;

/// Azure 应用注册的客户端 ID（公共客户端，无密钥；用户自行注册提供）
const CLIENT_ID: &str = "c7129530-5014-40a0-9559-50bcb10a3001";
/// 纯个人微软账号场景的标准端点（HMCL 同款做法；Minecraft 正版仅支持个人账号）
const MS_AUTH_BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
/// XboxLive.signin 进入 Xbox 认证体系；offline_access 拿 refresh_token（长期登录）
const SCOPE: &str = "XboxLive.signin offline_access";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_AUTH_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// 登录流程互斥（同一时间只允许一个设备码流程）
static LOGIN_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 取消标志（用户关闭弹窗 / 主动取消）
static LOGIN_CANCEL: AtomicBool = AtomicBool::new(false);

/// Drop 复位守卫：流程异常退出（含 panic）时保证互斥标志被释放
struct ResetLoginFlag;
impl Drop for ResetLoginFlag {
    fn drop(&mut self) {
        LOGIN_ACTIVE.store(false, Ordering::SeqCst);
        eprintln!("[ms-auth] 流程结束，互斥标志已复位");
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn client() -> Result<reqwest::Client, String> {
    crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent(concat!("PieMCL/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20)))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))
}

/// 用户主动取消当前登录流程
pub fn cancel_login() {
    LOGIN_CANCEL.store(true, Ordering::SeqCst);
}

// ---------------------------------------------------------------------------
// 设备码登录（批次 B 主流程）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DeviceCodeResp {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default)]
    expires_in: i64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
struct MsTokenResp {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

/// 发起设备码登录：立即返回，流程经 ms-login-state 事件推送前端
pub fn start_device_login(app: tauri::AppHandle) -> Result<(), String> {
    if LOGIN_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("已有登录流程在进行中，请先等待完成或取消".into());
    }
    LOGIN_CANCEL.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        // guard 在任务以任何方式退出（含 panic）时复位互斥标志
        let _guard = ResetLoginFlag;
        eprintln!("[ms-auth] 设备码流程启动");
        let result = device_login_flow(&app).await;
        match &result {
            Ok(()) => eprintln!("[ms-auth] 流程成功结束"),
            Err(e) => {
                eprintln!("[ms-auth] 流程失败：{e}");
                // 取消导致的失败不发事件（弹窗已关闭，避免误报）
                if !LOGIN_CANCEL.load(Ordering::SeqCst) {
                    let _ = app.emit_ms_login("failed", json!({ "message": e }));
                } else {
                    eprintln!("[ms-auth] 失败原因为用户取消，不推送事件");
                }
            }
        }
    });
    Ok(())
}

trait EmitMsLogin {
    fn emit_ms_login(&self, stage: &str, extra: Value) -> Result<(), tauri::Error>;
}

impl EmitMsLogin for tauri::AppHandle {
    fn emit_ms_login(&self, stage: &str, extra: Value) -> Result<(), tauri::Error> {
        let mut payload = json!({ "stage": stage });
        if let (Value::Object(obj), Value::Object(ex)) = (&mut payload, extra) {
            for (k, v) in ex {
                obj.insert(k, v);
            }
        }
        self.emit("ms-login-state", payload)
    }
}

/// 发送微软 OAuth 表单请求：非 2xx 时解析 error_description 给出中文提示
async fn post_microsoft_form(
    client: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<Value, String> {
    let resp = client
        .post(url)
        .form(form)
        .send()
        .await
        .map_err(|e| format!("微软请求失败：{e}（请检查网络后重试）"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取微软响应失败：{e}"))?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        let err_code = v
            .get("error")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        // 设备码轮询的业务态（HTTP 400 + error 字段）：交上层按 error 分支处理
        if err_code == "authorization_pending" || err_code == "slow_down" {
            return Ok(v);
        }
        let desc = v
            .get("error_description")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        // AADSTS70011：XboxLive.signin scope 未开通（应用审批未通过 / 未添加 API 权限）
        if desc.contains("AADSTS70011") {
            return Err(
                "该 Azure 应用尚未开通 XboxLive.signin 权限（微软审批未通过，或未在「API 权限」中添加）。\
                 请到 Azure 门户为应用添加 XboxLive.signin 委托权限并等待微软审批通过后重试"
                    .into(),
            );
        }
        return Err(format!("微软请求失败（{status} / {err_code}）：{desc}"));
    }
    Ok(v)
}

async fn device_login_flow(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let client = client()?;

    // ---- 1. 请求设备码 ----
    let code: DeviceCodeResp = serde_json::from_value(
        post_microsoft_form(
            &client,
            &format!("{MS_AUTH_BASE}/devicecode"),
            &[("client_id", CLIENT_ID), ("scope", SCOPE)],
        )
        .await?,
    )
    .map_err(|e| format!("设备码响应解析失败：{e}"))?;

    eprintln!(
        "[ms-auth] 已获取设备码 {}（interval={}s，expires_in={}s），浏览器已打开 {}",
        code.user_code, code.interval, code.expires_in, code.verification_uri
    );

    // 自动把设备码复制到系统剪贴板（失败不影响流程，前端保留手动复制兜底）
    let copied = match arboard::Clipboard::new().and_then(|mut c| c.set_text(&code.user_code)) {
        Ok(()) => {
            eprintln!("[ms-auth] 设备码已自动复制到剪贴板");
            true
        }
        Err(e) => {
            eprintln!("[ms-auth] 自动复制设备码失败（用户可手动复制）：{e}");
            false
        }
    };

    let _ = app.emit_ms_login(
        "code",
        json!({
            "userCode": code.user_code,
            "verifyUrl": code.verification_uri,
            "copied": copied,
        }),
    );
    // 打开系统浏览器（授权页固定为微软官方域名，非任意 URL）
    let _ = opener::open(&code.verification_uri);

    // ---- 2. 轮询令牌端点 ----
    let mut interval = code.interval.max(3);
    let deadline = now_secs() + code.expires_in.max(60);
    let ms = loop {
        if LOGIN_CANCEL.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
        if now_secs() > deadline {
            let _ = app.emit_ms_login("expired", json!({}));
            return Err("设备码已过期，请重新发起登录".into());
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if LOGIN_CANCEL.load(Ordering::SeqCst) {
            return Err("已取消".into());
        }
        let tr: MsTokenResp = serde_json::from_value(
            post_microsoft_form(
                &client,
                &format!("{MS_AUTH_BASE}/token"),
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", CLIENT_ID),
                    ("device_code", code.device_code.as_str()),
                ],
            )
            .await?,
        )
        .map_err(|e| format!("令牌响应解析失败：{e}"))?;
        if let Some(err) = &tr.error {
            eprintln!("[ms-auth] 轮询中：{}", err);
            match err.as_str() {
                // 用户尚未在浏览器完成授权：继续等
                "authorization_pending" => continue,
                // 请求过快：按微软建议加大间隔
                "slow_down" => {
                    interval += 5;
                    continue;
                }
                "authorization_declined" => {
                    let _ = app.emit_ms_login("denied", json!({}));
                    return Err("您在浏览器中拒绝了授权".into());
                }
                "expired_token" => {
                    let _ = app.emit_ms_login("expired", json!({}));
                    return Err("设备码已过期，请重新发起登录".into());
                }
                _ => {
                    let desc = tr.error_description.unwrap_or_else(|| err.clone());
                    return Err(format!("微软登录失败：{desc}"));
                }
            }
        }
        break tr;
    };

    let ms_refresh = ms
        .refresh_token
        .clone()
        .ok_or_else(|| {
            "微软未返回刷新令牌（请检查 Azure 应用是否勾选了 offline_access 权限）".to_string()
        })?;
    eprintln!(
        "[ms-auth] 已获取微软令牌（expires_in={}s，refresh_token={})",
        ms.expires_in,
        if ms.refresh_token.is_some() { "有" } else { "无" }
    );
    let _ = app.emit_ms_login("pending", json!({ "message": "授权成功，正在完成 Minecraft 认证…" }));

    // ---- 3. XBL / XSTS / MC 认证链 ----
    let chain = xbox_chain(&client, &ms.access_token).await?;
    eprintln!(
        "[ms-auth] 认证链完成（uhs={}，xuid={}，mc_token_expires_in={}s）",
        chain.uhs, chain.xuid, chain.mc_expires_in
    );

    // ---- 4. 拉取 profile（含皮肤披风，缓存到账户）----
    let profile = fetch_profile(&client, &chain.mc_token).await?;
    eprintln!(
        "[ms-auth] profile 已拉取：name={} id={}",
        profile.get("name").and_then(|x| x.as_str()).unwrap_or("?"),
        profile.get("id").and_then(|x| x.as_str()).unwrap_or("?")
    );

    // ---- 5. 建 / 更新账户并激活 ----
    let root = app
        .state::<crate::commands::AppState>()
        .paths
        .root()
        .to_path_buf();
    let saved = save_login_account(&root, &ms, &ms_refresh, &chain, &profile)?;
    eprintln!("[ms-auth] 账户已保存并激活：{}（{}）", saved.name, saved.id);

    let _ = app.emit_ms_login(
        "success",
        json!({ "message": format!("已登录正版账户「{}」", saved.name) }),
    );
    let _ = app.emit("accounts-changed", ());
    Ok(())
}

/// 把登录结果写入 accounts.json：按游戏 UUID 找已有微软账户则更新，否则新建；登录即激活
fn save_login_account(
    root: &Path,
    ms: &MsTokenResp,
    ms_refresh: &str,
    chain: &XboxAuth,
    profile: &Value,
) -> Result<Account, String> {
    let mut store = AccountStore::load(root);
    let game_uuid = format_mc_uuid(
        profile
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or("profile 缺少 id 字段")?,
    );
    let name = profile
        .get("name")
        .and_then(|x| x.as_str())
        .unwrap_or("Player")
        .to_string();

    let existing = store
        .accounts
        .iter_mut()
        .find(|a| a.kind == "microsoft" && a.uuid == game_uuid);
    let account = match existing {
        Some(a) => {
            a.name = name.clone();
            a.ms_access_token = Some(ms.access_token.clone());
            a.ms_refresh_token = Some(ms_refresh.to_string());
            a.ms_expires_at = Some(now_secs() + ms.expires_in.max(0));
            a.mc_access_token = Some(chain.mc_token.clone());
            a.mc_expires_at = Some(now_secs() + chain.mc_expires_in.max(0));
            a.uhs = Some(chain.uhs.clone());
            a.xuid = Some(chain.xuid.clone());
            a.skins = parse_skins(profile);
            a.capes = parse_capes(profile);
            a.active_cape_id = profile
                .pointer("/capes")
                .and_then(|x| x.as_array())
                .and_then(|arr| arr.iter().find(|c| c["state"] == "ACTIVE"))
                .and_then(|c| c["id"].as_str())
                .map(String::from);
            a.clone()
        }
        None => {
            let account = Account {
                id: uuid::Uuid::new_v4().simple().to_string(),
                kind: "microsoft".into(),
                name: name.clone(),
                uuid: game_uuid,
                created_at: now_secs(),
                ms_access_token: Some(ms.access_token.clone()),
                ms_refresh_token: Some(ms_refresh.to_string()),
                ms_expires_at: Some(now_secs() + ms.expires_in.max(0)),
                mc_access_token: Some(chain.mc_token.clone()),
                mc_expires_at: Some(now_secs() + chain.mc_expires_in.max(0)),
                uhs: Some(chain.uhs.clone()),
                xuid: Some(chain.xuid.clone()),
                auth_server_url: None,
                auth_api_root: None,
                auth_server_name: None,
                auth_access_token: None,
                skins: parse_skins(profile),
                capes: parse_capes(profile),
                active_cape_id: profile
                    .pointer("/capes")
                    .and_then(|x| x.as_array())
                    .and_then(|arr| arr.iter().find(|c| c["state"] == "ACTIVE"))
                    .and_then(|c| c["id"].as_str())
                    .map(String::from),
                offline_skin: None,
            };
            store.accounts.push(account.clone());
            account
        }
    };
    store.set_active(&account.id)?;
    store.save(root)?;
    Ok(account)
}

/// Mojang profile 的 UUID 无连字符 → 启动参数需要的标准 8-4-4-4-12 形式
fn format_mc_uuid(raw: &str) -> String {
    let hex: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return raw.to_string();
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// 解析 profile 里的皮肤列表（供 profile 模块刷新缓存复用）
pub(crate) fn parse_skins(profile: &Value) -> Vec<SkinEntry> {
    profile
        .get("skins")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| {
                    Some(SkinEntry {
                        id: s.get("id")?.as_str()?.to_string(),
                        state: s.get("state").and_then(|x| x.as_str()).unwrap_or("").into(),
                        url: s.get("url").and_then(|x| x.as_str()).unwrap_or("").into(),
                        variant: s
                            .get("variant")
                            .and_then(|x| x.as_str())
                            .unwrap_or("classic")
                            .to_ascii_lowercase(),
                        alias: s.get("alias").and_then(|x| x.as_str()).map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 解析 profile 里的披风列表（供 profile 模块刷新缓存复用）
pub(crate) fn parse_capes(profile: &Value) -> Vec<CapeEntry> {
    profile
        .get("capes")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(CapeEntry {
                        id: c.get("id")?.as_str()?.to_string(),
                        state: c.get("state").and_then(|x| x.as_str()).unwrap_or("").into(),
                        alias: c.get("alias").and_then(|x| x.as_str()).map(String::from),
                        url: c.get("url").and_then(|x| x.as_str()).unwrap_or("").into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// XBL / XSTS / Minecraft 认证链（登录与刷新共用）
// ---------------------------------------------------------------------------

/// 认证链结果
struct XboxAuth {
    mc_token: String,
    mc_expires_in: i64,
    uhs: String,
    xuid: String,
}

/// 单次 XSTS 授权请求（RelyingParty 可变，供降级重试）
async fn xsts_attempt(
    client: &reqwest::Client,
    relying_party: &str,
    xbl_token: &str,
) -> Result<(reqwest::StatusCode, String), String> {
    let resp = client
        .post(XSTS_URL)
        .json(&json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token],
            },
            "RelyingParty": relying_party,
            "TokenType": "JWT",
        }))
        .send()
        .await
        .map_err(|e| format!("XSTS 认证请求失败：{e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取 XSTS 响应失败：{e}"))?;
    Ok((status, text))
}

async fn xbox_chain(client: &reqwest::Client, ms_access_token: &str) -> Result<XboxAuth, String> {
    // ---- XBL：RPS 票据换用户令牌 ----
    eprintln!("[ms-auth] 步骤 1/3：请求 XBL…");
    let r: Value = client
        .post(XBL_URL)
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access_token}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Xbox Live 认证失败：{e}"))?
        .json()
        .await
        .map_err(|e| format!("XBL 响应解析失败：{e}"))?;
    let xbl_token = r
        .get("Token")
        .and_then(|x| x.as_str())
        .ok_or("XBL 响应缺少 Token")?
        .to_string();
    let uhs = r
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(|x| x.as_str())
        .ok_or("XBL 响应缺少 uhs")?
        .to_string();
    eprintln!(
        "[ms-auth] XBL Token 已获取（长度={}，前缀={}…）",
        xbl_token.len(),
        &xbl_token[..xbl_token.len().min(12)]
    );

    // ---- XSTS：换授权令牌（非 2xx 时响应体带 XErr，说明具体原因）----
    // RelyingParty 存在两种取值：通用授权与 Minecraft 专用；部分账号对前者返回
    // 400 空响应体，此时自动降级用 MC 专用 RP 重试一次
    eprintln!("[ms-auth] 步骤 1/3 完成，步骤 2/3：请求 XSTS…");
    let (mut status, mut xsts_text) =
        xsts_attempt(client, "http://auth.xboxlive.com", &xbl_token).await?;
    if !status.is_success() {
        eprintln!(
            "[ms-auth] XSTS 标准 RP 被拒（{status} {}），尝试 Minecraft 专用 RelyingParty…",
            xsts_text.chars().take(200).collect::<String>()
        );
        let (s2, t2) = xsts_attempt(client, "rp://api.minecraftservices.com/", &xbl_token).await?;
        status = s2;
        xsts_text = t2;
    }
    eprintln!(
        "[ms-auth] XSTS 响应：{status} {}",
        xsts_text.chars().take(400).collect::<String>()
    );
    if !status.is_success() {
        let body: Value = serde_json::from_str(&xsts_text).unwrap_or(Value::Null);
        // XErr 可能是数字或字符串数字
        let xerr = body
            .get("XErr")
            .and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok())));
        if let Some(code) = xerr {
            return Err(xerr_message(Some(code)));
        }
        return Err(format!(
            "XSTS 认证失败（{status}）：{}",
            if xsts_text.is_empty() {
                "无响应体".to_string()
            } else {
                xsts_text.chars().take(300).collect()
            }
        ));
    }
    let r2: Value = serde_json::from_str(&xsts_text)
        .map_err(|e| format!("XSTS 响应解析失败：{e}"))?;
    let xsts_token = r2
        .get("Token")
        .and_then(|x| x.as_str())
        .ok_or("XSTS 响应缺少 Token")?
        .to_string();
    // DisplayClaims 字段名为 xid（部分历史文档写 xuid，两者兼容读取）
    let xuid = r2
        .pointer("/DisplayClaims/xui/0/xid")
        .or_else(|| r2.pointer("/DisplayClaims/xui/0/xuid"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    // ---- Minecraft Services：XSTS 令牌换 MC access_token ----
    eprintln!("[ms-auth] 步骤 2/3 完成，步骤 3/3：请求 Minecraft Services…");
    let r3: Value = client
        .post(MC_AUTH_URL)
        .json(&json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}"),
        }))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Minecraft 认证失败：{e}"))?
        .json()
        .await
        .map_err(|e| format!("MC 认证响应解析失败：{e}"))?;
    let mc_token = r3
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or("MC 认证响应缺少 access_token")?
        .to_string();
    let mc_expires_in = r3.get("expires_in").and_then(|x| x.as_i64()).unwrap_or(86400);
    eprintln!("[ms-auth] 步骤 3/3 完成（mc_expires_in={mc_expires_in}s）");

    Ok(XboxAuth {
        mc_token,
        mc_expires_in,
        uhs,
        xuid,
    })
}

/// XSTS XErr 错误码 → 中文提示
fn xerr_message(xerr: Option<i64>) -> String {
    match xerr {
        Some(2148916233) => "该微软账号未拥有 Minecraft（未购买或未关联）".into(),
        Some(2148916235) => "该账号所在国家/地区不支持 Xbox Live".into(),
        Some(2148916236) => "该账号需要完成成人身份验证后才能登录".into(),
        Some(2148916237) => "该账号需要完成儿童身份验证后才能登录".into(),
        Some(2148916238) => "该账号属于未成年人账户，无法访问此服务（需在家庭组中授权）".into(),
        Some(2148916227) => "该账号已被 Xbox 平台封禁，无法登录".into(),
        Some(code) => format!("Xbox 授权失败（XErr {code}）"),
        None => "Xbox 授权失败（未知错误）".into(),
    }
}

/// 拉取 MC profile（404 = 未购买 Java 版；供 profile 模块复用）
pub(crate) async fn fetch_profile(client: &reqwest::Client, mc_token: &str) -> Result<Value, String> {
    let resp = client
        .get(MC_PROFILE_URL)
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| format!("拉取 Minecraft 资料失败：{e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("该微软账号未拥有 Minecraft Java 版".into());
    }
    resp.error_for_status()
        .map_err(|e| format!("拉取 Minecraft 资料失败：{e}"))?
        .json()
        .await
        .map_err(|e| format!("profile 响应解析失败：{e}"))
}

// ---------------------------------------------------------------------------
// 启动前静默刷新（launch_instance 调用）
// ---------------------------------------------------------------------------

/// 确保账户持有有效 MC token：未过期直接返回；否则用 refresh_token 走完整刷新链。
/// 刷新成功后更新 accounts.json（token / profile 缓存）；refresh_token 失效时提示重新登录。
pub async fn ensure_mc_token(account: &Account, root: &Path) -> Result<String, String> {
    // ---- 1. MC token 仍有效：直接用缓存 ----
    let now = now_secs();
    let mc_valid = account
        .mc_access_token
        .as_deref()
        .map(|t| !t.is_empty())
        .unwrap_or(false)
        && account.mc_expires_at.map(|e| e - now > 60).unwrap_or(false)
        && account.uhs.is_some();
    if mc_valid {
        return Ok(account.mc_access_token.clone().unwrap_or_default());
    }

    // ---- 2. 刷新微软 token → 重新走认证链 ----
    let refresh = account
        .ms_refresh_token
        .clone()
        .ok_or_else(|| "登录状态已失效，请到「账户管理」页重新登录".to_string())?;
    let client = client()?;
    let ms: MsTokenResp = client
        .post(format!("{MS_AUTH_BASE}/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", CLIENT_ID),
            ("refresh_token", refresh.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| "登录状态已失效，请到「账户管理」页重新登录微软账户".to_string())?
        .json()
        .await
        .map_err(|e| format!("刷新令牌响应解析失败：{e}"))?;
    if let Some(err) = &ms.error {
        // refresh_token 被吊销 / 过期：只能重新登录
        return Err(format!(
            "登录状态已失效（{err}），请到「账户管理」页重新登录微软账户"
        ));
    }
    let ms_refresh = ms
        .refresh_token
        .clone()
        .unwrap_or(refresh); // 微软刷新时可能不返回新 refresh_token，沿用旧的

    let chain = xbox_chain(&client, &ms.access_token).await?;
    let profile = fetch_profile(&client, &chain.mc_token).await?;

    // ---- 3. 写回账户（token + profile 缓存）----
    let mut store = AccountStore::load(root);
    let updated = {
        let acc = store
            .get_mut(&account.id)
            .ok_or("账户已被移除，请重新登录")?;
        acc.name = profile
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(&acc.name)
            .to_string();
        acc.ms_access_token = Some(ms.access_token.clone());
        acc.ms_refresh_token = Some(ms_refresh);
        acc.ms_expires_at = Some(now_secs() + ms.expires_in.max(0));
        acc.mc_access_token = Some(chain.mc_token.clone());
        acc.mc_expires_at = Some(now_secs() + chain.mc_expires_in.max(0));
        acc.uhs = Some(chain.uhs.clone());
        acc.xuid = Some(chain.xuid.clone());
        acc.skins = parse_skins(&profile);
        acc.capes = parse_capes(&profile);
        acc.active_cape_id = profile
            .pointer("/capes")
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.iter().find(|c| c["state"] == "ACTIVE"))
            .and_then(|c| c["id"].as_str())
            .map(String::from);
        acc.clone()
    };
    store.save(root)?;

    Ok(updated.mc_access_token.unwrap_or_default())
}

/// 确保账户持有有效 MC token 并组装启动用认证参数（launch_instance 调用）
pub async fn ensure_mc_auth(account: &Account, root: &Path) -> Result<AuthContext, String> {
    let token = ensure_mc_token(account, root).await?;
    // 刷新路径下账户已写回，从磁盘取最新 uuid / xuid
    let acc = AccountStore::load(root)
        .get(&account.id)
        .cloned()
        .unwrap_or_else(|| account.clone());
    Ok(AuthContext {
        uuid: acc.uuid,
        access_token: token,
        user_type: "msa".into(),
        xuid: acc.xuid.unwrap_or_default(),
    })
}
