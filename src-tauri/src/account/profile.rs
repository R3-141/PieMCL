// 正版账户皮肤 / 披风管理（Minecraft Services API，批次 C）
//
// - 皮肤上传/切换：POST /minecraft/profile/skins，multipart 表单
//   （variant = slim/classic + file = PNG）。注意：老文档为 PUT + JSON
//   （支持 url/file 字段），实测（2026-10）PUT/PATCH 均 405、JSON 格式 403，
//   且 multipart 仅认 file 字段（url 字段报 "Missing skin file in request"）。
//   因此「历史皮肤切换 / 换模型 / URL 应用」统一改为：从皮肤 url 下载 PNG
//   后以 file 重传，等效达成原 url 提交语义。
// - 披风：PUT /minecraft/profile/capes/active 装备 / 脱下（capeId = null）。
//   注意：老文档写的是 PATCH，实测（2026-10）PATCH/POST 均返回 405，
//   仅 PUT 被服务端接受
// 所有操作后重新拉取 profile 刷新账户缓存并写回 accounts.json。

use std::path::Path;

use serde_json::{Value, json};

use super::ms_auth;
use super::store::AccountStore;
use super::Account;

const PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SKINS_URL: &str = "https://api.minecraftservices.com/minecraft/profile/skins";
const CAPES_ACTIVE_URL: &str = "https://api.minecraftservices.com/minecraft/profile/capes/active";

/// 校验 PNG 魔数与大小上限（1MB）
fn validate_png(data: &[u8]) -> Result<(), String> {
    if data.len() > 1_048_576 {
        return Err("皮肤文件过大（需小于 1MB）".into());
    }
    if data.len() < 8 || data[..8] != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return Err("不是有效的 PNG 文件".into());
    }
    Ok(())
}

/// 从皮肤 url 下载 PNG（Mojang textures CDN 可能返回 http:// 地址）
async fn download_skin_png(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载皮肤文件失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("下载皮肤文件失败：{e}"))?;
    let data = resp
        .bytes()
        .await
        .map_err(|e| format!("读取皮肤文件失败：{e}"))?
        .to_vec();
    validate_png(&data)?;
    Ok(data)
}

/// POST multipart 上传皮肤（官方 2026-10 现行为：仅接受 file 字段）
async fn post_skin_multipart(
    client: &reqwest::Client,
    mc_token: &str,
    variant: &str,
    png: Vec<u8>,
) -> Result<(), String> {
    let form = reqwest::multipart::Form::new()
        .text("variant", variant.to_string())
        .part(
            "file",
            reqwest::multipart::Part::bytes(png)
                .file_name("skin.png")
                .mime_str("image/png")
                .map_err(|e| format!("构造皮肤请求失败：{e}"))?,
        );
    let resp = client
        .post(SKINS_URL)
        .bearer_auth(mc_token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("应用皮肤请求失败：{e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(skins_error(status, &body));
    }
    Ok(())
}

/// 操作后刷新账户的 profile 缓存（skins / capes / activeCapeId / name）
async fn refresh_cache(
    root: &Path,
    client: &reqwest::Client,
    account_id: &str,
    mc_token: &str,
) -> Result<(), String> {
    let profile = ms_auth::fetch_profile(client, mc_token).await?;
    let mut store = AccountStore::load(root);
    {
        let acc = store
            .get_mut(account_id)
            .ok_or("账户已被移除，请重新登录")?;
        if let Some(name) = profile.get("name").and_then(|x| x.as_str()) {
            acc.name = name.to_string();
        }
        acc.skins = ms_auth::parse_skins(&profile);
        acc.capes = ms_auth::parse_capes(&profile);
        acc.active_cape_id = profile
            .pointer("/capes")
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.iter().find(|c| c["state"] == "ACTIVE"))
            .and_then(|c| c["id"].as_str())
            .map(String::from);
    }
    store.save(root)
}

/// PUT skins 的统一错误提示（400 常见于皮肤文件无效或未拥有）
fn skins_error(status: reqwest::StatusCode, body: &str) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return "登录状态已失效，请到「账户管理」页重新登录微软账户".into();
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return "操作过于频繁，Minecraft Services 有速率限制，请稍后再试".into();
    }
    let detail = body.chars().take(200).collect::<String>();
    if detail.is_empty() {
        format!("皮肤操作失败（{status}）")
    } else {
        format!("皮肤操作失败（{status}）：{detail}")
    }
}

/// 应用皮肤库中的历史皮肤（下载该皮肤的 url 后以 file 重传）
pub async fn apply_library_skin(
    root: &Path,
    account: &Account,
    skin_id: &str,
) -> Result<(), String> {
    let skin = account
        .skins
        .iter()
        .find(|s| s.id == skin_id)
        .ok_or("皮肤不存在或已被移除，请刷新后重试")?;
    if skin.state == "ACTIVE" {
        return Err("该皮肤已是当前皮肤".into());
    }
    if skin.url.is_empty() {
        return Err("该皮肤没有可用的下载地址".into());
    }
    let mc_token = ms_auth::ensure_mc_token(account, root).await?;
    let client = ms_auth::client()?;
    let png = download_skin_png(&client, &skin.url).await?;
    post_skin_multipart(&client, &mc_token, &skin.variant, png).await?;
    refresh_cache(root, &client, &account.id, &mc_token).await
}

/// 用当前皮肤换模型（纤细/粗壮）：下载 ACTIVE 皮肤后以新 variant 重传。
/// variant：slim / classic
pub async fn switch_skin_variant(
    root: &Path,
    account: &Account,
    variant: &str,
) -> Result<(), String> {
    if variant != "slim" && variant != "classic" {
        return Err(format!("未知的皮肤模型：{variant}"));
    }
    let active = account
        .skins
        .iter()
        .find(|s| s.state == "ACTIVE")
        .ok_or("未找到当前皮肤记录，请先刷新或重新登录")?;
    if active.url.is_empty() {
        return Err("当前皮肤没有可用的下载地址，无法切换模型".into());
    }
    let mc_token = ms_auth::ensure_mc_token(account, root).await?;
    let client = ms_auth::client()?;
    let png = download_skin_png(&client, &active.url).await?;
    post_skin_multipart(&client, &mc_token, variant, png).await?;
    refresh_cache(root, &client, &account.id, &mc_token).await
}

/// 上传本地 PNG 皮肤。variant：slim / classic
pub async fn upload_skin(
    root: &Path,
    account: &Account,
    png_path: &str,
    variant: &str,
) -> Result<(), String> {
    if variant != "slim" && variant != "classic" {
        return Err(format!("未知的皮肤模型：{variant}"));
    }
    let data = tokio::fs::read(png_path)
        .await
        .map_err(|e| format!("读取皮肤文件失败：{e}"))?;
    validate_png(&data)?;
    let mc_token = ms_auth::ensure_mc_token(account, root).await?;
    let client = ms_auth::client()?;
    post_skin_multipart(&client, &mc_token, variant, data).await?;
    refresh_cache(root, &client, &account.id, &mc_token).await
}

/// 以 URL 应用皮肤：下载该 url 的 PNG 后以 file 重传
/// （用户手动提供的地址要求 https；皮肤换源下载放行 http 以兼容 textures CDN）
pub async fn apply_skin_url(
    root: &Path,
    account: &Account,
    url: &str,
    variant: &str,
) -> Result<(), String> {
    if variant != "slim" && variant != "classic" {
        return Err(format!("未知的皮肤模型：{variant}"));
    }
    let url = url.trim();
    if !url.starts_with("https://") {
        return Err("皮肤地址必须以 https:// 开头（官方接口要求）".into());
    }
    let mc_token = ms_auth::ensure_mc_token(account, root).await?;
    let client = ms_auth::client()?;
    let png = download_skin_png(&client, url).await?;
    post_skin_multipart(&client, &mc_token, variant, png).await?;
    refresh_cache(root, &client, &account.id, &mc_token).await
}

/// 装备披风
pub async fn equip_cape(root: &Path, account: &Account, cape_id: &str) -> Result<(), String> {
    if !account.capes.iter().any(|c| c.id == cape_id) {
        return Err("未拥有该披风，请刷新后重试".into());
    }
    let mc_token = ms_auth::ensure_mc_token(account, root).await?;
    let client = ms_auth::client()?;
    let resp = client
        .put(CAPES_ACTIVE_URL)
        .bearer_auth(&mc_token)
        .json(&json!({ "capeId": cape_id }))
        .send()
        .await
        .map_err(|e| format!("装备披风请求失败：{e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(skins_error(status, &body));
    }
    refresh_cache(root, &client, &account.id, &mc_token).await
}

/// 脱下披风（capeId 传 null）
pub async fn unequip_cape(root: &Path, account: &Account) -> Result<(), String> {
    let mc_token = ms_auth::ensure_mc_token(account, root).await?;
    let client = ms_auth::client()?;
    let resp = client
        .put(CAPES_ACTIVE_URL)
        .bearer_auth(&mc_token)
        .json(&json!({ "capeId": Value::Null }))
        .send()
        .await
        .map_err(|e| format!("脱下披风请求失败：{e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(skins_error(status, &body));
    }
    refresh_cache(root, &client, &account.id, &mc_token).await
}
