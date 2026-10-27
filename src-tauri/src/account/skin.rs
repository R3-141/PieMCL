// 离线皮肤抓取与存储（批次 D）
//
// 皮肤来源（多入口，统一落 skins/{account_id}.png + accounts.json 元数据）：
// - player：正版玩家名抓取（Mojang API → sessionserver textures → PNG 下载）
// - file：  本地 PNG 文件（弹窗选择）
// - url：   网络地址直接下载
// - preset：内置官方默认皮肤（Steve / Alex，打包进二进制，离线可用）
// 注意 Mojang API 有速率限制（600 次/10 分钟），单人使用无碍，超限有中文提示。

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;

use super::store::AccountStore;
use super::OfflineSkin;

const MOJANG_PROFILE_API: &str = "https://api.mojang.com/users/profiles/minecraft";
const SESSION_SERVER: &str = "https://sessionserver.mojang.com/session/minecraft/profile";

/// 内置官方默认皮肤（64x64，构建期固化，完全离线可用）
/// Steve = 经典模型（classic）；Alex = 纤细模型（slim）
const STEVE_PNG: &[u8] = include_bytes!("../../assets/skins/steve.png");
const ALEX_PNG: &[u8] = include_bytes!("../../assets/skins/alex.png");

/// 皮肤文件大小上限（64x64 PNG 远小于此值，防误传超大文件）
const SKIN_MAX_BYTES: usize = 1024 * 1024;

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

/// 校验皮肤 PNG：魔数 + IHDR 尺寸必须为 64x64（新版）或 64x32（旧版）。
/// 手写解析 PNG 头（宽高在固定偏移 16–24 字节，大端），零依赖
pub fn validate_skin_png(png: &[u8]) -> Result<(), String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if png.len() > SKIN_MAX_BYTES {
        return Err(format!(
            "皮肤文件过大（{} KB，上限 1MB）",
            png.len() / 1024
        ));
    }
    if png.len() < 24 || png[..8] != SIG {
        return Err("不是有效的 PNG 图片文件".to_string());
    }
    // 8 签名 + 4 长度 + 4 "IHDR" 之后是宽高（各 4 字节大端）
    if &png[12..16] != b"IHDR" {
        return Err("PNG 文件缺少 IHDR 头（已损坏）".to_string());
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    match (width, height) {
        (64, 64) | (64, 32) => Ok(()),
        _ => Err(format!(
            "皮肤尺寸 {width}x{height} 不受支持（需为 64x64 或 64x32 的皮肤贴图）"
        )),
    }
}

/// 账户当前模型（已有皮肤沿用其 variant；否则 classic）
fn current_variant(store: &AccountStore, account_id: &str) -> String {
    store
        .get(account_id)
        .and_then(|a| a.offline_skin.as_ref())
        .map(|s| s.variant.clone())
        .unwrap_or_else(|| "classic".to_string())
}

/// 统一应用入口：写 PNG + 写元数据（所有来源共用）
async fn apply_skin_bytes(
    root: &Path,
    account_id: &str,
    png: &[u8],
    source: &str,
    source_name: &str,
    variant: &str,
) -> Result<(), String> {
    validate_skin_png(png)?;
    let png_path = skin_png_path(root, account_id);
    tokio::fs::create_dir_all(png_path.parent().unwrap())
        .await
        .map_err(|e| format!("创建皮肤目录失败：{e}"))?;
    tokio::fs::write(&png_path, png)
        .await
        .map_err(|e| format!("保存皮肤文件失败：{e}"))?;

    let mut store = AccountStore::load(root);
    let acc = store
        .get_mut(account_id)
        .ok_or("账户不存在或已被移除")?;
    acc.offline_skin = Some(OfflineSkin {
        source: source.to_string(),
        source_name: source_name.to_string(),
        variant: variant.to_string(),
        fetched_at: now_secs(),
    });
    store.save(root)
}

/// 应用内置预设皮肤（preset：steve / alex），模型随预设固定
pub async fn apply_preset(root: &Path, account_id: &str, preset: &str) -> Result<(), String> {
    let (png, variant, name) = match preset.to_ascii_lowercase().as_str() {
        "steve" => (STEVE_PNG, "classic", "Steve"),
        "alex" => (ALEX_PNG, "slim", "Alex"),
        _ => return Err(format!("未知的预设皮肤：{preset}（可选 Steve / Alex）")),
    };
    apply_skin_bytes(root, account_id, png, "preset", name, variant).await
}

/// 应用 URL 皮肤（仅 http/https；模型沿用当前设置）
pub async fn apply_url_skin(root: &Path, account_id: &str, url: &str) -> Result<(), String> {
    let url = url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("皮肤地址需以 http:// 或 https:// 开头".to_string());
    }
    let resp = client()?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载皮肤失败：{e}"))?;
    let resp = resp.error_for_status().map_err(|e| format!("下载皮肤失败：{e}"))?;
    let png = resp
        .bytes()
        .await
        .map_err(|e| format!("读取皮肤文件失败：{e}"))?
        .to_vec();
    if png.is_empty() {
        return Err("下载到的皮肤文件为空".into());
    }
    let store = AccountStore::load(root);
    let variant = current_variant(&store, account_id);
    // source_name 截断展示（URL 可能极长）
    let label: String = url.chars().take(64).collect();
    apply_skin_bytes(root, account_id, &png, "url", &label, &variant).await
}

/// 应用本地文件皮肤（模型沿用当前设置）
pub async fn apply_file_skin(root: &Path, account_id: &str, path: &Path) -> Result<(), String> {
    let png = tokio::fs::read(path)
        .await
        .map_err(|e| format!("读取皮肤文件失败：{e}"))?;
    if png.is_empty() {
        return Err("皮肤文件为空".into());
    }
    let store = AccountStore::load(root);
    let variant = current_variant(&store, account_id);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "本地文件".to_string());
    apply_skin_bytes(root, account_id, &png, "file", &name, &variant).await
}

/// 抓取并应用到离线账户（写 PNG + accounts.json 元数据）
pub async fn fetch_and_apply(root: &Path, account_id: &str, player_name: &str) -> Result<(), String> {
    let fetched = fetch_player_skin(player_name).await?;
    apply_skin_bytes(
        root,
        account_id,
        &fetched.png,
        "player",
        &fetched.source_name,
        &fetched.variant,
    )
    .await
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
    // 同步刷新时间戳：fetchedAt 会拼进注入纹理 URL 作缓存击穿参数，
    // 只换模型时也要让 URL 变化，确保游戏重新拉取带新模型声明的纹理
    meta.fetched_at = now_secs();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造最小 PNG 头（签名 + IHDR 宽高），尺寸校验只需前 24 字节
    fn fake_png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v
    }

    #[test]
    fn skin_png_accepts_both_layouts() {
        assert!(validate_skin_png(&fake_png(64, 64)).is_ok());
        assert!(validate_skin_png(&fake_png(64, 32)).is_ok());
        // 内置预设皮肤必须是合法规格（防止贴图被误换后无人发现）
        assert!(validate_skin_png(STEVE_PNG).is_ok());
        assert!(validate_skin_png(ALEX_PNG).is_ok());
    }

    #[test]
    fn skin_png_rejects_bad_input() {
        // 非法尺寸
        assert!(validate_skin_png(&fake_png(128, 128)).is_err());
        assert!(validate_skin_png(&fake_png(32, 32)).is_err());
        // 非 PNG
        assert!(validate_skin_png(b"GIF89a-anything").is_err());
        // 截断
        assert!(validate_skin_png(&fake_png(64, 64)[..16]).is_err());
        // 空
        assert!(validate_skin_png(&[]).is_err());
    }
}
