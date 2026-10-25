// 离线皮肤抓取与存储（批次 D）
//
// 抓取链：正版玩家名 → Mojang API UUID → sessionserver textures → 皮肤 PNG 下载
// 存储：{root}/skins/{account_id}.png + accounts.json 的 offline_skin 元数据
// 注意 Mojang API 有速率限制（600 次/10 分钟），单人使用无碍，超限有中文提示。

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;

use super::store::AccountStore;
use super::OfflineSkin;

const MOJANG_PROFILE_API: &str = "https://api.mojang.com/users/profiles/minecraft";
const SESSION_SERVER: &str = "https://sessionserver.mojang.com/session/minecraft/profile";

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn client() -> Result<reqwest::Client, String> {
    crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent(concat!("PieMCL/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30)))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))
}

/// 抓取结果
pub struct FetchedSkin {
    pub png: Vec<u8>,
    pub variant: String,
    pub source_name: String,
}

/// 按正版玩家名抓取皮肤（PNG + 模型）
pub async fn fetch_player_skin(player_name: &str) -> Result<FetchedSkin, String> {
    let name = player_name.trim();
    if name.is_empty() || name.len() > 16 {
        return Err("请输入合法的正版玩家名（不超过 16 个字符）".into());
    }
    let client = client()?;

    // ---- 1. 玩家名 → UUID ----
    let url = format!("{MOJANG_PROFILE_API}/{name}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("查询玩家资料失败：{e}"))?;
    if resp.status() == reqwest::StatusCode::NO_CONTENT
        || resp.status() == reqwest::StatusCode::NOT_FOUND
    {
        return Err(format!("未找到正版玩家「{name}」"));
    }
    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Mojang API 请求过于频繁，请一分钟后再试".into());
    }
    let uuid_raw = resp
        .error_for_status()
        .map_err(|e| format!("查询玩家资料失败：{e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("玩家资料解析失败：{e}"))?
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or("玩家资料缺少 id 字段")?
        .to_string();

    // ---- 2. UUID → textures（unsigned=false 取完整属性）----
    let url = format!("{SESSION_SERVER}/{uuid_raw}?unsigned=false");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("拉取皮肤纹理失败：{e}"))?;
    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Mojang 会话服务器请求过于频繁，请一分钟后再试".into());
    }
    let profile = resp
        .error_for_status()
        .map_err(|e| format!("拉取皮肤纹理失败：{e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("皮肤纹理响应解析失败：{e}"))?;
    let textures_b64 = profile
        .get("properties")
        .and_then(|x| x.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("name").and_then(|x| x.as_str()) == Some("textures"))
        })
        .and_then(|p| p.get("value"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| format!("该玩家未设置自定义皮肤（无纹理数据）"))?;
    let textures_json: Value = serde_json::from_slice(
        &base64::engine::general_purpose::STANDARD
            .decode(textures_b64)
            .map_err(|e| format!("纹理数据解码失败：{e}"))?,
    )
    .map_err(|e| format!("纹理数据解析失败：{e}"))?;

    // ---- 3. 解析 SKIN（url + 模型）----
    let skin = textures_json
        .pointer("/textures/SKIN")
        .ok_or("该玩家未设置自定义皮肤")?;
    let skin_url = skin
        .get("url")
        .and_then(|x| x.as_str())
        .ok_or("皮肤纹理缺少下载地址")?
        .to_string();
    let variant = skin
        .pointer("/meta/model")
        .and_then(|x| x.as_str())
        .map(|m| m.to_ascii_lowercase())
        .unwrap_or_else(|| "classic".into());

    // ---- 4. 下载皮肤 PNG ----
    let resp = client
        .get(&skin_url)
        .send()
        .await
        .map_err(|e| format!("下载皮肤文件失败：{e}"))?;
    let png = resp
        .error_for_status()
        .map_err(|e| format!("下载皮肤文件失败：{e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取皮肤文件失败：{e}"))?
        .to_vec();
    if png.is_empty() {
        return Err("下载到的皮肤文件为空".into());
    }

    Ok(FetchedSkin {
        png,
        variant,
        source_name: profile
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(name)
            .to_string(),
    })
}

/// 皮肤 PNG 存放路径：{root}/skins/{account_id}.png
pub fn skin_png_path(root: &Path, account_id: &str) -> PathBuf {
    root.join("skins").join(format!("{account_id}.png"))
}

/// 抓取并应用到离线账户（写 PNG + accounts.json 元数据）
pub async fn fetch_and_apply(root: &Path, account_id: &str, player_name: &str) -> Result<(), String> {
    let fetched = fetch_player_skin(player_name).await?;
    let png_path = skin_png_path(root, account_id);
    tokio::fs::create_dir_all(png_path.parent().unwrap())
        .await
        .map_err(|e| format!("创建皮肤目录失败：{e}"))?;
    tokio::fs::write(&png_path, &fetched.png)
        .await
        .map_err(|e| format!("保存皮肤文件失败：{e}"))?;

    let mut store = AccountStore::load(root);
    let acc = store
        .get_mut(account_id)
        .ok_or("账户不存在或已被移除")?;
    acc.offline_skin = Some(OfflineSkin {
        source_name: fetched.source_name,
        variant: fetched.variant,
        fetched_at: now_secs(),
    });
    store.save(root)
}

/// 切换离线皮肤模型（slim / classic；贴图不变，仅改模型声明）
pub async fn set_variant(root: &Path, account_id: &str, variant: &str) -> Result<(), String> {
    if variant != "slim" && variant != "classic" {
        return Err(format!("未知的皮肤模型：{variant}"));
    }
    let mut store = AccountStore::load(root);
    let acc = store
        .get_mut(account_id)
        .ok_or("账户不存在或已被移除")?;
    let Some(meta) = acc.offline_skin.as_mut() else {
        return Err("尚未设置离线皮肤".into());
    };
    if meta.variant == variant {
        return Ok(());
    }
    meta.variant = variant.to_string();
    store.save(root)
}

/// 移除离线账户皮肤（删 PNG + 清元数据）
pub async fn remove(root: &Path, account_id: &str) -> Result<(), String> {
    let png_path = skin_png_path(root, account_id);
    let _ = tokio::fs::remove_file(&png_path).await;
    let mut store = AccountStore::load(root);
    if let Some(acc) = store.get_mut(account_id) {
        acc.offline_skin = None;
    }
    store.save(root)
}

/// 读取皮肤 PNG（本地会话服务用）
pub fn read_skin_png(root: &Path, account_id: &str) -> Option<Vec<u8>> {
    std::fs::read(skin_png_path(root, account_id)).ok()
}

/// 读取皮肤 PNG 的 data URL（前端 3D 预览用）
pub async fn read_skin_data_url(root: &Path, account_id: &str) -> Result<Option<String>, String> {
    let path = skin_png_path(root, account_id);
    match tokio::fs::read(&path).await {
        Ok(data) => Ok(Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(data)
        ))),
        Err(_) => Ok(None),
    }
}
