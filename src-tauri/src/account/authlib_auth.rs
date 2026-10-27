// 皮肤站认证（authlib-injector 规范，Blessing Skin 系站点通用）
//
// 认证流程（authlib-injector API 规范）：
//   1. GET {server}/authlib-injector/api/yggdrasil → 站点元数据
//      （响应头 x-authlib-injector-api-location 指向真实 API 根时跟随）
//   2. POST {api_root}/authserver/authenticate {username,password}
//      → accessToken + availableProfiles（皮肤站普遍支持一号多角色）
//   3. POST {api_root}/authserver/validate {accessToken} → 204 有效 / 401 失效
//   4. POST {api_root}/authserver/refresh {accessToken, selectedProfile} → 新 accessToken
//   5. POST {api_root}/authserver/invalidate {accessToken} → 吊销（登出）
//
// 密码不落盘；accessToken 明文存 accounts.json（与微软 token 同一存储决策）。
// 启动注入：-javaagent:authlib-injector.jar={server_url}，agent 自行拉取元数据，
// 启动器侧认证请求统一走 api_root（authenticate 等端点不在 server 根下）。
// 所有请求 reqwest 直连（项目约定：不跟随系统代理）。

use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};

/// 皮肤站元数据（登录前预览用）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerMeta {
    /// 真实 API 根（如 https://littleskin.cn/api/yggdrasil）
    pub api_root: String,
    /// 站点名（meta.serverName，缺失时回落 host）
    pub server_name: String,
    /// 注册页链接（meta.links.register）
    #[serde(default)]
    pub register_url: Option<String>,
    /// 主页链接（meta.links.homepage）
    #[serde(default)]
    pub homepage_url: Option<String>,
}

/// 皮肤站角色（availableProfiles 条目）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    /// 角色 UUID（无连字符 hex，原样保存）
    pub id: String,
    /// 角色名
    pub name: String,
}

fn client() -> Result<reqwest::Client, String> {
    crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent(concat!("PieMCL/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20)))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))
}

/// 规范化服务器地址：补协议、去尾斜杠；仅允许 http(s)
pub fn normalize_server_url(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("请输入皮肤站服务器地址".into());
    }
    let with_scheme = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    let base = with_scheme.trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("服务器地址需为 http(s) 链接".into());
    }
    Ok(base.to_string())
}

/// 从站点元数据 JSON 解析 ServerMeta（api_root 已在外部按响应头确定）
fn parse_server_meta(value: &Value, api_root: String, fallback_name: &str) -> ServerMeta {
    let server_name = value
        .pointer("/meta/serverName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback_name)
        .to_string();
    let link = |key: &str| {
        value
            .pointer(&format!("/meta/links/{key}"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
    };
    ServerMeta {
        api_root,
        server_name,
        register_url: link("register"),
        homepage_url: link("homepage"),
    }
}

/// 解析认证响应（authenticate / refresh / oauth 共用）：token + 角色列表 + 站点预选角色
pub(crate) fn parse_authenticate_resp(
    value: &Value,
) -> Result<(String, Vec<ProfileInfo>, Option<ProfileInfo>), String> {
    // 站点错误统一格式：{ error, errorMessage }（errorMessage 为中文提示时直接透传）
    if let Some(msg) = value.get("errorMessage").and_then(|v| v.as_str()) {
        return Err(msg.to_string());
    }
    let token = value
        .get("accessToken")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "皮肤站响应缺少 accessToken".to_string())?
        .to_string();
    let profiles = value
        .get("availableProfiles")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let id = p.get("id")?.as_str()?.to_string();
                    let name = p.get("name")?.as_str()?.to_string();
                    Some(ProfileInfo { id, name })
                })
                .collect()
        })
        .unwrap_or_default();
    let selected = value.get("selectedProfile").and_then(|p| {
        Some(ProfileInfo {
            id: p.get("id")?.as_str()?.to_string(),
            name: p.get("name")?.as_str()?.to_string(),
        })
    });
    Ok((token, profiles, selected))
}

/// 规范化角色 UUID：无连字符 hex → 连字符标准形式（游戏启动参数需要）
pub fn dashed_uuid(hex: &str) -> Result<String, String> {
    let clean: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.len() != 32 {
        return Err(format!("角色 UUID 格式异常：{hex}"));
    }
    Ok(uuid::Uuid::parse_str(&clean)
        .map_err(|e| format!("角色 UUID 解析失败：{e}"))?
        .to_string())
}

/// 从相对/绝对 ALI 值解析出 API 根
fn resolve_ali(loc: &str, base: &str) -> String {
    if loc.starts_with("http://") || loc.starts_with("https://") {
        return loc.trim_end_matches('/').to_string();
    }
    let origin = base
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or_default())
        .unwrap_or_default();
    let scheme = base.split("://").next().unwrap_or("https");
    format!("{scheme}://{origin}{}", if loc.starts_with('/') { loc.to_string() } else { format!("/{loc}") })
}

/// 获取皮肤站元数据。探测候选端点（部分站点如 LittleSkin 不提供
/// /authlib-injector 探测路径，直接 404，需回落 /api/yggdrasil），
/// 响应携带 x-authlib-injector-api-location（ALI）时跟随至真实 API 根。
pub async fn fetch_server_meta(server_url: &str) -> Result<ServerMeta, String> {
    let base = normalize_server_url(server_url)?;
    let candidates = [
        format!("{base}/authlib-injector/api/yggdrasil"),
        format!("{base}/api/yggdrasil"),
    ];
    let http = client()?;
    let mut last_status: Option<u16> = None;
    for url in &candidates {
        let resp = http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("连接皮肤站失败：{e}"))?;
        if !resp.status().is_success() {
            last_status = Some(resp.status().as_u16());
            continue; // 404 等状态尝试下一候选端点
        }
        // ALI 跟随：相对路径（如 /api/yggdrasil）按请求 URL 解析；绝对 URL 直接采用
        let api_root = resp
            .headers()
            .get("x-authlib-injector-api-location")
            .and_then(|v| v.to_str().ok())
            .map(|loc| resolve_ali(loc, &base))
            .unwrap_or_else(|| url.trim_end_matches('/').to_string());
        let value: Value = resp.json().await.map_err(|e| format!("解析皮肤站元数据失败：{e}"))?;
        let fallback_name = base.split("://").nth(1).unwrap_or(&base).to_string();
        return Ok(parse_server_meta(&value, api_root, &fallback_name));
    }
    Err(match last_status {
        Some(s) => format!("皮肤站返回异常状态：{s}（请确认地址为有效的 authlib-injector 皮肤站）"),
        None => "连接皮肤站失败：未找到 Yggdrasil API 端点".into(),
    })
}

/// 账密登录：返回 (站点元数据, accessToken, 全部角色, 站点预选角色)
pub async fn login(server_url: &str, username: &str, password: &str) -> Result<(ServerMeta, String, Vec<ProfileInfo>, Option<ProfileInfo>), String> {
    let meta = fetch_server_meta(server_url).await?;
    let resp = client()?
        .post(format!("{}/authserver/authenticate", meta.api_root))
        // agent 为 Yggdrasil 登录规范字段（LittleSkin 等站点强校验：缺失报
        // "agent must be an object"，name 非 "Minecraft" 报 "name must be equal to Minecraft"）
        .json(&json!({
            "agent": { "name": "Minecraft", "version": 1 },
            "username": username.trim(),
            "password": password,
        }))
        .send()
        .await
        .map_err(|e| format!("登录请求失败：{e}"))?;
    let status = resp.status();
    let value: Value = resp.json().await.map_err(|e| format!("解析登录响应失败：{e}"))?;
    parse_authenticate_resp(&value)
        .map(|(t, p, s)| (meta.clone(), t, p, s))
        .map_err(|e| {
            if status.as_u16() == 403 || status.as_u16() == 400 {
                e // errorMessage 已透传（含密码错误 / 2FA 提示）
            } else {
                format!("登录失败（HTTP {status}）：{e}")
            }
        })
}

/// 校验 token 是否有效（204 有效；401 / 403 / 其他视为无效）
pub async fn validate(api_root: &str, token: &str) -> bool {
    match client() {
        Ok(c) => matches!(
            c.post(format!("{api_root}/authserver/validate"))
                .json(&json!({ "accessToken": token }))
                .send()
                .await,
            Ok(r) if r.status().as_u16() == 204
        ),
        Err(_) => false,
    }
}

/// 刷新 token：携带当前角色续期，返回新 accessToken
pub async fn refresh(api_root: &str, token: &str, profile: &ProfileInfo) -> Result<String, String> {
    let resp = client()?
        .post(format!("{api_root}/authserver/refresh"))
        .json(&json!({
            "accessToken": token,
            "selectedProfile": { "id": profile.id, "name": profile.name },
        }))
        .send()
        .await
        .map_err(|e| format!("刷新皮肤站令牌失败：{e}"))?;
    let status = resp.status();
    let value: Value = resp.json().await.map_err(|e| format!("解析刷新响应失败：{e}"))?;
    parse_authenticate_resp(&value)
        .map(|(t, _, _)| t)
        .map_err(|e| format!("刷新皮肤站令牌失败（HTTP {status}）：{e}"))
}

/// 吊销 token（登出 / 移除账户时 best-effort，失败静默）
pub async fn invalidate(api_root: &str, token: &str) {
    if let Ok(c) = client() {
        let api_root = api_root.to_string();
        let token = token.to_string();
        tauri::async_runtime::spawn(async move {
            let _ = c
                .post(format!("{api_root}/authserver/invalidate"))
                .json(&json!({ "accessToken": token }))
                .send()
                .await;
        });
    }
}

// ---------------------------------------------------------------------------
// 启动前认证保障 + 移除账户吊销
// ---------------------------------------------------------------------------

use std::path::Path;

use super::store::AccountStore;
use super::Account;
use crate::launch::args::AuthContext;

/// 创建皮肤站账户记录（kind="authlib"；入库与去重由调用方负责）
pub fn new_account(
    server_url: &str,
    api_root: &str,
    server_name: &str,
    token: &str,
    profile: &ProfileInfo,
) -> Result<Account, String> {
    Ok(Account {
        id: uuid::Uuid::new_v4().simple().to_string(),
        kind: "authlib".into(),
        name: profile.name.clone(),
        uuid: dashed_uuid(&profile.id)?,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        ms_access_token: None,
        ms_refresh_token: None,
        ms_expires_at: None,
        mc_access_token: None,
        mc_expires_at: None,
        uhs: None,
        xuid: None,
        auth_server_url: Some(server_url.to_string()),
        auth_api_root: Some(api_root.to_string()),
        auth_server_name: Some(server_name.to_string()),
        auth_access_token: Some(token.to_string()),
        skins: Vec::new(),
        capes: Vec::new(),
        active_cape_id: None,
        offline_skin: None,
    })
}

/// 启动前认证保障：validate 现有 token，失效则 refresh 续期（写回 accounts.json），
/// 两者皆失败（密码已改 / token 被吊销）时中止启动并提示重新登录。
pub async fn ensure_authlib_auth(account: &Account, root: &Path) -> Result<AuthContext, String> {
    let api_root = account
        .auth_api_root
        .clone()
        .ok_or("皮肤站账户数据不完整，请删除该账户后重新登录")?;
    let token = account
        .auth_access_token
        .clone()
        .filter(|t| !t.is_empty())
        .ok_or("皮肤站登录已过期，请删除该账户后重新登录")?;
    // refresh 的 selectedProfile 使用站点签发的无连字符 UUID
    let profile = ProfileInfo {
        id: account.uuid.replace('-', ""),
        name: account.name.clone(),
    };
    let token = if validate(&api_root, &token).await {
        token
    } else {
        eprintln!("[authlib] token 已失效，尝试 refresh：{}", account.name);
        let new_token = refresh(&api_root, &token, &profile).await?;
        let account_id = account.id.clone();
        let root_owned = root.to_path_buf();
        let write_token = new_token.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
            let mut store = AccountStore::load(&root_owned);
            if let Some(acc) = store.get_mut(&account_id) {
                acc.auth_access_token = Some(write_token);
            }
            store.save(&root_owned)
        })
        .await
        .map_err(|e| format!("账户写入任务失败：{e}"))??;
        eprintln!("[authlib] token 已刷新并写回");
        new_token
    };
    Ok(AuthContext {
        uuid: account.uuid.clone(),
        access_token: token,
        // authlib-injector 体系沿用传统 Mojang 会话语义
        user_type: "mojang".into(),
        xuid: String::new(),
    })
}

/// 移除账户时吊销站点 token（best-effort，不阻塞删除）
pub fn revoke_on_remove(account: &Account) {
    if account.is_authlib() {
        if let (Some(api_root), Some(token)) =
            (account.auth_api_root.as_deref(), account.auth_access_token.as_deref())
        {
            let api_root = api_root.to_string();
            let token = token.to_string();
            tauri::async_runtime::spawn(async move {
                invalidate(&api_root, &token).await;
            });
        }
    }
}

/// 皮肤站角色当前皮肤（3D 预览用）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinData {
    /// PNG 的 data URL
    pub data_url: String,
    /// classic / slim（textures 元数据 model，缺省 classic）
    pub model: String,
}

/// 拉取皮肤站角色的当前皮肤贴图：
/// {api_root}/sessionserver/session/minecraft/profile/{uuid} → textures 属性
/// → base64 解码出 SKIN.url → 下载 PNG。无皮肤 / 站点不可达时返回 None。
pub async fn fetch_skin_data(account: &Account) -> Result<Option<SkinData>, String> {
    let api_root = account
        .auth_api_root
        .as_deref()
        .ok_or("皮肤站账户数据不完整")?;
    let uuid_nodash = account.uuid.replace('-', "");
    let c = client()?;
    let resp = c
        .get(format!("{api_root}/sessionserver/session/minecraft/profile/{uuid_nodash}"))
        .send()
        .await
        .map_err(|e| format!("查询皮肤站角色资料失败：{e}"))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let value: Value = resp.json().await.map_err(|e| format!("解析角色资料失败：{e}"))?;
    // properties 中 name == "textures" 的 base64 value
    let tex_b64 = value
        .pointer("/properties")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))
        })
        .and_then(|p| p.get("value"))
        .and_then(|v| v.as_str())
        .ok_or("角色资料缺少 textures 属性")?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(tex_b64)
        .map_err(|e| format!("textures 解码失败：{e}"))?;
    let tex: Value = serde_json::from_slice(&decoded).map_err(|e| format!("textures 解析失败：{e}"))?;
    let Some(skin_url) = tex.pointer("/textures/SKIN/url").and_then(|v| v.as_str()) else {
        return Ok(None); // 未设置皮肤
    };
    let model = tex
        .pointer("/textures/SKIN/metadata/model")
        .and_then(|v| v.as_str())
        .unwrap_or("classic")
        .to_string();
    let png = c
        .get(skin_url)
        .send()
        .await
        .map_err(|e| format!("下载皮肤贴图失败：{e}"))?
        .bytes()
        .await
        .map_err(|e| format!("下载皮肤贴图失败：{e}"))?;
    Ok(Some(SkinData {
        data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        ),
        model,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_server_url_rules() {
        assert_eq!(normalize_server_url(" littleskin.cn ").unwrap(), "https://littleskin.cn");
        assert_eq!(normalize_server_url("https://littleskin.cn/").unwrap(), "https://littleskin.cn");
        assert_eq!(normalize_server_url("http://127.0.0.1:8080").unwrap(), "http://127.0.0.1:8080");
        assert!(normalize_server_url("").is_err());
        assert!(normalize_server_url("ftp://x").is_err());
    }

    #[test]
    fn dashed_uuid_rules() {
        let dashed = dashed_uuid("d3088894d65d3d6fb86d67b3fd6f6f34").unwrap();
        assert_eq!(dashed.len(), 36);
        assert_eq!(dashed.replace('-', ""), "d3088894d65d3d6fb86d67b3fd6f6f34");
        assert!(dashed_uuid("").is_err());
        assert!(dashed_uuid("short").is_err());
    }

    #[test]
    fn parse_authenticate_resp_shapes() {
        let ok = json!({
            "accessToken": "tok",
            "availableProfiles": [
                { "id": "aa3088894d65d3d6fb86d67b3fd6f6f3", "name": "Alice" },
                { "id": "bb3088894d65d3d6fb86d67b3fd6f6f3", "name": "Bob" }
            ]
        });
        let (token, profiles, selected) = parse_authenticate_resp(&ok).unwrap();
        assert_eq!(token, "tok");
        assert_eq!(profiles.len(), 2);
        assert!(selected.is_none());

        let err = json!({ "error": "ForbiddenOperationException", "errorMessage": "密码错误" });
        assert_eq!(parse_authenticate_resp(&err).unwrap_err(), "密码错误");
    }

    #[test]
    fn parse_server_meta_fields() {
        let v = json!({
            "meta": {
                "serverName": "LittleSkin",
                "links": { "register": "https://littleskin.cn/auth/register", "homepage": "https://littleskin.cn" }
            }
        });
        let meta = parse_server_meta(&v, "https://littleskin.cn/api/yggdrasil".into(), "littleskin.cn");
        assert_eq!(meta.server_name, "LittleSkin");
        assert_eq!(meta.register_url.as_deref(), Some("https://littleskin.cn/auth/register"));
        // 缺失字段回落
        let empty = parse_server_meta(&json!({}), "x".into(), "fallback.host");
        assert_eq!(empty.server_name, "fallback.host");
        assert!(empty.register_url.is_none());
    }
}
