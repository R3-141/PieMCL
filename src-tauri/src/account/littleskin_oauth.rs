// LittleSkin OAuth 2 设备码流登录（LittleSkin 站点特有扩展）
//
// 与通用账密登录（authlib_auth.rs）并存：仅 LittleSkin 域名显示免密授权入口，
// 其他 Blessing Skin 站点走账密回落。流程（manual.littlesk.in/advanced/oauth2）：
//   1. POST open.littleskin.cn/oauth/device_code (client_id + scope) → 设备码对
//   2. 浏览器打开授权页，用户输入授权码（user_code）并选择角色
//   3. 轮询 POST open.littleskin.cn/oauth/token → access_token + id_token
//      （id_token.payload.selectedProfile = 用户在授权页所选角色）
//   4. POST {api_root}/authserver/oauth {uuid} + Bearer → Minecraft 令牌
//      响应为标准 Yggdrasil 登录格式，签发的令牌可直接经 Yggdrasil API
//      validate / refresh / invalidate（与账密签发令牌同一套链路）
//
// 状态经 authlib-oauth-state 事件推送（code / pending / success / failed），
// 交互与微软设备码登录（ms_auth.rs）同款。client_id 为 LittleSkin OAuth
// 应用分配的公共客户端 ID；设备码流需站点白名单，未过审时返回 invalid_client，
// 前端提示改用账密登录。
//
// ID 令牌说明：JWT 直接从令牌端点经 TLS 获取，此处仅解码 payload 并校验
// aud（受众为本应用），不做 JWKS 签名验证（本地启动器场景攻击面有限，
// 签发方与传输通道均受 LittleSkin 控制）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};
use tauri::Emitter;

use super::authlib_auth::{self, ProfileInfo};
use super::store::AccountStore;

/// LittleSkin OAuth 应用（公共客户端，设备码流无需 secret）
const OAUTH_CLIENT_ID: &str = "1658";
/// LittleSkin OpenAPI / OAuth 服务基础地址（与站点根不同域）
const OAUTH_BASE: &str = "https://open.littleskin.cn";
/// openid：返回 ID 令牌；Select：授权页选角色；Create：签发 Minecraft 令牌
const SCOPE: &str = "openid Yggdrasil.PlayerProfiles.Select Yggdrasil.MinecraftToken.Create";
const LITTLESKIN_HOST: &str = "littleskin.cn";

/// 登录流程互斥（同一时间只允许一个设备码流程）
static LOGIN_ACTIVE: AtomicBool = AtomicBool::new(false);
/// 取消标志
static LOGIN_CANCEL: AtomicBool = AtomicBool::new(false);

/// Drop 复位守卫：流程异常退出（含 panic）时保证互斥标志被释放
struct ResetLoginFlag;
impl Drop for ResetLoginFlag {
    fn drop(&mut self) {
        LOGIN_ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// 判断站点是否为 LittleSkin（支持子域）
pub fn is_littleskin(server_url: &str) -> bool {
    let lower = server_url.to_ascii_lowercase();
    let host = lower.split("://").nth(1).unwrap_or(&lower);
    let host = host.split('/').next().unwrap_or("");
    host == LITTLESKIN_HOST || host.ends_with(&format!(".{LITTLESKIN_HOST}"))
}

/// 用户主动取消当前登录流程
pub fn cancel_login() {
    LOGIN_CANCEL.store(true, Ordering::SeqCst);
}

fn emit_state(app: &tauri::AppHandle, stage: &str, extra: Value) {
    let mut payload = json!({ "stage": stage });
    if let (Value::Object(obj), Value::Object(ex)) = (&mut payload, extra) {
        for (k, v) in ex {
            obj.insert(k, v);
        }
    }
    let _ = app.emit("authlib-oauth-state", payload);
}

/// 发起设备码登录：立即返回，流程经 authlib-oauth-state 事件推送前端
pub fn start_device_login(app: tauri::AppHandle, server_url: String, root: PathBuf) -> Result<(), String> {
    if !is_littleskin(&server_url) {
        return Err("该站点不支持免密授权登录，请使用账号密码登录".into());
    }
    if LOGIN_ACTIVE.swap(true, Ordering::SeqCst) {
        return Err("已有登录流程在进行中，请先等待完成或取消".into());
    }
    LOGIN_CANCEL.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let _guard = ResetLoginFlag;
        eprintln!("[ls-oauth] 设备码流程启动");
        let result = device_login_flow(&app, &server_url, &root).await;
        match &result {
            Ok(()) => eprintln!("[ls-oauth] 流程成功结束"),
            Err(e) => {
                eprintln!("[ls-oauth] 流程失败：{e}");
                // 取消导致的失败不发事件（弹窗已关闭，避免误报）
                if !LOGIN_CANCEL.load(Ordering::SeqCst) {
                    emit_state(&app, "failed", json!({ "message": e }));
                }
            }
        }
    });
    Ok(())
}

/// 发送 OAuth 表单请求：非 2xx 时解析 error 字段（设备码轮询的业务态交上层分支处理）
async fn post_form(
    client: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<Value, String> {
    let resp = client
        .post(url)
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(|e| format!("LittleSkin 请求失败：{e}（请检查网络后重试）"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取 LittleSkin 响应失败：{e}"))?;
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if !status.is_success() {
        let err_code = v.get("error").and_then(|x| x.as_str()).unwrap_or("").to_string();
        // 设备码轮询的业务态（HTTP 400 + error 字段）：交上层按 error 分支处理
        if err_code == "authorization_pending" || err_code == "slow_down" {
            return Ok(v);
        }
        let desc = v
            .get("error_description")
            .or_else(|| v.get("errorMessage"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if err_code == "invalid_client" {
            return Err(
                "该应用尚未通过 LittleSkin 设备码流白名单审核，暂不可用免密授权；\
                 请改用下方账号密码登录"
                    .into(),
            );
        }
        return Err(format!("LittleSkin 请求失败（{status} / {err_code}）：{desc}"));
    }
    Ok(v)
}

/// 解码 ID 令牌 payload 并校验受众（不做签名验证，见模块注释）
fn parse_id_token(id_token: &str) -> Result<Value, String> {
    let payload_b64 = id_token
        .split('.')
        .nth(1)
        .ok_or("ID 令牌格式异常（缺少 payload 段）")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(payload_b64))
        .map_err(|e| format!("ID 令牌解码失败：{e}"))?;
    let claims: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("ID 令牌解析失败：{e}"))?;
    let aud_ok = claims
        .get("aud")
        .map(|v| match v {
            Value::String(s) => s == OAUTH_CLIENT_ID,
            Value::Array(arr) => arr.iter().any(|x| x.as_str() == Some(OAUTH_CLIENT_ID)),
            _ => false,
        })
        .unwrap_or(false);
    if !aud_ok {
        return Err("ID 令牌受众校验失败（aud 与应用不符）".into());
    }
    Ok(claims)
}

async fn device_login_flow(
    app: &tauri::AppHandle,
    server_url: &str,
    root: &std::path::Path,
) -> Result<(), String> {
    let client = crate::account::ms_auth::client()?;

    // ---- 1. 请求设备码对 ----
    let code = post_form(
        &client,
        &format!("{OAUTH_BASE}/oauth/device_code"),
        &[("client_id", OAUTH_CLIENT_ID), ("scope", SCOPE)],
    )
    .await?;
    let user_code = code
        .get("user_code")
        .and_then(|v| v.as_str())
        .ok_or("设备码响应缺少 user_code")?
        .to_string();
    let device_code = code
        .get("device_code")
        .and_then(|v| v.as_str())
        .ok_or("设备码响应缺少 device_code")?
        .to_string();
    // 授权页固定为 LittleSkin 官方域名，可直接打开（优先带码完整链接）
    let verify_url = code
        .get("verification_uri_complete")
        .or_else(|| code.get("verification_uri"))
        .and_then(|v| v.as_str())
        .unwrap_or("https://open.littleskin.cn/oauth/link")
        .to_string();
    let interval = code.get("interval").and_then(|v| v.as_u64()).unwrap_or(5).max(3);
    let expires_in = code.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(300);

    eprintln!(
        "[ls-oauth] 已获取设备码 {user_code}（interval={interval}s，expires_in={expires_in}s）"
    );

    // 自动复制授权码（失败不影响流程，前端保留手动复制兜底）
    let copied = match arboard::Clipboard::new().and_then(|mut c| c.set_text(&user_code)) {
        Ok(()) => true,
        Err(_) => false,
    };
    emit_state(app, "code", json!({ "userCode": user_code, "verifyUrl": verify_url, "copied": copied }));
    let _ = opener::open(&verify_url);

    // ---- 2. 轮询令牌端点 ----
    let deadline =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
            + expires_in.max(30) as u64;
    let mut interval = interval;
    let mut access_token = String::new();
    let mut id_token = Option::<String>::None;
    loop {
        if LOGIN_CANCEL.load(Ordering::SeqCst) {
            return Err("登录已取消".into());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now >= deadline {
            return Err("授权码已过期，请重新发起登录".into());
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if LOGIN_CANCEL.load(Ordering::SeqCst) {
            return Err("登录已取消".into());
        }
        let resp = post_form(
            &client,
            &format!("{OAUTH_BASE}/oauth/token"),
            &[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", OAUTH_CLIENT_ID),
                ("device_code", device_code.as_str()),
            ],
        )
        .await?;
        match resp.get("error").and_then(|v| v.as_str()) {
            Some("authorization_pending") => {
                emit_state(app, "pending", json!({ "message": "等待你在浏览器中完成授权…" }));
                continue;
            }
            Some("slow_down") => {
                interval += 5;
                continue;
            }
            Some("expired_token") => return Err("授权码已过期，请重新发起登录".into()),
            Some(e) => return Err(format!("LittleSkin 授权失败：{e}")),
            None => {}
        }
        access_token = resp
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or("令牌响应缺少 access_token")?
            .to_string();
        id_token = resp
            .get("id_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        break;
    }

    // ---- 3. ID 令牌 → 用户所选角色 ----
    let claims = parse_id_token(
        id_token.as_deref().ok_or("未返回 ID 令牌（缺少 openid 权限？）")?,
    )?;
    let selected = claims
        .get("selectedProfile")
        .ok_or("授权结果缺少所选角色（未申请 Yggdrasil.PlayerProfiles.Select？）")?;
    let profile = ProfileInfo {
        id: selected
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("所选角色缺少 id")?
            .to_string(),
        name: selected
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("所选角色缺少 name")?
            .to_string(),
    };

    // ---- 4. 换取 Minecraft 令牌（标准 Yggdrasil 登录响应格式）----
    emit_state(app, "pending", json!({ "message": "正在获取游戏令牌…" }));
    let meta = authlib_auth::fetch_server_meta(server_url).await?;
    let mc_resp = client
        .post(format!("{}/authserver/oauth", meta.api_root))
        .header("Accept", "application/json")
        .bearer_auth(&access_token)
        .json(&json!({ "uuid": profile.id.replace('-', "") }))
        .send()
        .await
        .map_err(|e| format!("获取 Minecraft 令牌失败：{e}"))?;
    let status = mc_resp.status();
    let mc_value: Value = mc_resp
        .json()
        .await
        .map_err(|e| format!("解析 Minecraft 令牌响应失败：{e}"))?;
    let (mc_token, _profiles, mc_selected) =
        authlib_auth::parse_authenticate_resp(&mc_value).map_err(|e| {
            if status.as_u16() == 403 {
                format!("获取 Minecraft 令牌被拒绝：{e}")
            } else {
                format!("获取 Minecraft 令牌失败（HTTP {status}）：{e}")
            }
        })?;
    // 以令牌响应中的 selectedProfile 为准（与授权页选择一致）
    let final_profile = mc_selected.unwrap_or(profile);

    // ---- 5. 入库（去重 + 激活）----
    let root2 = root.to_path_buf();
    let server_url = server_url.to_string();
    let account = tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
        let mut store = AccountStore::load(&root2);
        let account = authlib_auth::new_account(
            &server_url,
            &meta.api_root,
            &meta.server_name,
            &mc_token,
            &final_profile,
        )?;
        if store.accounts.iter().any(|a| a.is_authlib() && a.uuid == account.uuid) {
            return Err(format!("已存在皮肤站角色「{}」，无需重复添加", account.name));
        }
        let view = account.view();
        store.accounts.push(account);
        let new_id = store.accounts.last().unwrap().id.clone();
        store.set_active(&new_id)?;
        store.save(&root2)?;
        Ok(view)
    })
    .await
    .map_err(|e| format!("账户写入任务失败：{e}"))??;

    emit_state(
        app,
        "success",
        json!({ "message": format!("已添加皮肤站角色「{}」并设为当前账户", account.name) }),
    );
    Ok(())
}
