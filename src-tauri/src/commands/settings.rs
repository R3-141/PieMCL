// 设置命令：读取 / 保存全局设置（镜像源、并发数）
//
// 说明：并发数在引擎创建时生效，修改后需重启应用；
// 镜像列表实时生效（每次构建下载候选 URL 时读取）。

use tauri::State;

use crate::commands::AppState;
use crate::core::settings::AppSettings;

/// 读取全局设置
#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.settings.read().await.clone())
}

/// 校验并保存全局设置，返回规范化后的设置（前端据此同步表单）
#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let mut s = settings;
    s.validate()?;
    s.save(&state.paths).await?;
    // 同步全局代理配置（请求级 Client 立即生效；引擎常驻 Client 重启生效）
    crate::core::net::init_proxy(crate::core::net::ProxyConfig {
        mode: s.proxy_mode.clone(),
        url: s.proxy_url.clone(),
    });
    // 同步模组市场源运行时镜像（modrinth / curseforge / mirror 模块读取）
    crate::core::settings::sync_market_sources(&s);
    *state.settings.write().await = s.clone();
    Ok(s)
}

/// 检查更新：查询 GitHub Releases 最新正式版并与当前版本比较。
/// force=true 为设置页手动检查（忽略跳过版本与启动开关）；否则为启动静默检查
/// ——未启用启动检查或最新版已被用户跳过时，返回 has_update=false 形态（不弹窗）。
#[tauri::command]
pub async fn check_update(
    state: State<'_, AppState>,
    force: bool,
) -> Result<crate::core::update::UpdateCheckResult, String> {
    let result = crate::core::update::check_update().await.map_err(String::from)?;
    if !force {
        let (enabled, skipped) = {
            let s = state.settings.read().await;
            (s.update_check_enabled, s.update_skipped_version.clone())
        };
        if !enabled || result.latest_version == skipped {
            return Ok(crate::core::update::UpdateCheckResult {
                has_update: false,
                ..result
            });
        }
    }
    Ok(result)
}

/// 当前数据目录（实例、Java、皮肤、账户与设置所在根目录）
#[tauri::command]
pub fn get_data_dir(state: State<'_, AppState>) -> String {
    state.paths.root().display().to_string()
}

/// 在系统文件管理器中打开数据目录
#[tauri::command]
pub fn open_data_dir(state: State<'_, AppState>) -> Result<(), String> {
    opener::open(state.paths.root()).map_err(|e| format!("打开文件夹失败：{e}"))
}

/// 在系统文件管理器中定位任意已存在的文件 / 目录（Java 管理等页面用）
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    let p = std::path::Path::new(path.trim());
    if !p.exists() {
        return Err(format!("路径不存在：{path}"));
    }
    if p.is_dir() {
        opener::open(p).map_err(|e| format!("打开目录失败：{e}"))
    } else {
        opener::reveal(p).map_err(|e| format!("打开位置失败：{e}"))
    }
}

/// 用系统默认程序打开外部链接（「关于」页的仓库 / 主页 / 邮箱）。
/// 仅放行 http / https / mailto，防任意协议注入
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    let ok = url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("mailto:");
    if !ok {
        return Err(format!("不支持打开的链接：{url}"));
    }
    opener::open(&url).map_err(|e| format!("打开链接失败：{e}"))
}

/// 清空市场缓存（模组/资源图标代理缓存 cache\icons\），返回删除的文件数。
/// 清除后重新进入市场 / 资源页面即会全量重新拉取
#[tauri::command]
pub async fn clear_market_cache(state: State<'_, AppState>) -> Result<u64, String> {
    let dir = state.paths.cache_dir().join("icons");
    tokio::task::spawn_blocking(move || {
        let mut n = 0u64;
        let Ok(read) = std::fs::read_dir(&dir) else {
            return Ok(0u64);
        };
        for e in read.flatten() {
            let p = e.path();
            if p.is_file() {
                match std::fs::remove_file(&p) {
                    Ok(()) => n += 1,
                    Err(err) => eprintln!("[cache] 清除图标缓存失败 {}：{err}", p.display()),
                }
            }
        }
        Ok(n)
    })
    .await
    .map_err(|e| format!("清除任务执行失败：{e}"))?
}

// ---------------- 自定义背景 ----------------

/// 背景图允许的扩展名（前端裁剪后统一输出 JPEG，png/webp 兼容保留）
const BACKGROUND_EXTS: &[&str] = &["jpg", "png", "webp"];

fn background_mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

/// 保存裁剪后的背景图（覆盖旧文件并启用），返回规范化后的设置
#[tauri::command]
pub async fn save_background_image(
    state: State<'_, AppState>,
    bytes: Vec<u8>,
    original: Vec<u8>,
    ext: String,
) -> Result<AppSettings, String> {
    let ext = ext.to_ascii_lowercase();
    if !BACKGROUND_EXTS.contains(&ext.as_str()) {
        return Err(format!("不支持的背景图格式：{ext}（仅支持 jpg / png / webp）"));
    }
    if bytes.is_empty() {
        return Err("背景图数据为空".to_string());
    }
    if bytes.len() > 12 * 1024 * 1024 {
        return Err("背景图过大（超过 12MB），请裁剪或压缩后再试".to_string());
    }
    let dir = state.paths.background_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("创建背景目录失败：{e}"))?;
    // 清理旧扩展名文件（每次仅保留当前一张）
    for old in BACKGROUND_EXTS {
        if *old != ext {
            let _ = tokio::fs::remove_file(state.paths.background_path(old)).await;
        }
    }
    let path = state.paths.background_path(&ext);
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| format!("保存背景图失败：{e}"))?;
    // 同存未裁剪原图（供「重新裁剪」取景）
    if !original.is_empty() {
        let orig_path = state.paths.background_dir().join(format!("original.{ext}"));
        tokio::fs::write(&orig_path, &original)
            .await
            .map_err(|e| format!("保存原图失败：{e}"))?;
    }

    let mut s = state.settings.read().await.clone();
    s.background_image = format!("bg.{ext}");
    s.validate()?;
    s.save(&state.paths).await?;
    crate::core::settings::sync_market_sources(&s);
    *state.settings.write().await = s.clone();
    Ok(s)
}

/// 读取背景图为 data URL；未启用或文件缺失返回 None（静默回退无背景）
#[tauri::command]
pub async fn read_background_image(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let name = state.settings.read().await.background_image.clone();
    if name.is_empty() {
        return Ok(None);
    }
    // 文件名固定模式 bg.{ext}，取 ext 定位文件（防设置被手改出路径穿越）
    let ext = name.rsplit('.').next().unwrap_or("");
    if !BACKGROUND_EXTS.contains(&ext) {
        return Ok(None);
    }
    let path = state.paths.background_path(ext);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            use base64::Engine as _;
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(Some(format!("data:{};base64,{}", background_mime(ext), b64)))
        }
        // 文件被手动删除：回退无背景，不报错
        Err(_) => Ok(None),
    }
}

/// 移除自定义背景（删文件 + 清空设置），返回规范化后的设置
#[tauri::command]
pub async fn remove_background_image(state: State<'_, AppState>) -> Result<AppSettings, String> {
    for ext in BACKGROUND_EXTS {
        let _ = tokio::fs::remove_file(state.paths.background_path(ext)).await;
        let _ = tokio::fs::remove_file(state.paths.background_dir().join(format!("original.{ext}")))
            .await;
    }
    let mut s = state.settings.read().await.clone();
    s.background_image.clear();
    s.validate()?;
    s.save(&state.paths).await?;
    crate::core::settings::sync_market_sources(&s);
    *state.settings.write().await = s.clone();
    Ok(s)
}

/// 弹出文件对话框选择背景图，读取为 data URL（取消返回 None）
#[tauri::command]
pub async fn pick_background_image(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use base64::Engine as _;
    use tauri_plugin_dialog::DialogExt;

    let app2 = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .add_filter("图片", &["png", "jpg", "jpeg", "webp"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("对话框任务执行失败：{e}"))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| format!("获取所选路径失败：{e}"))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return Err("不支持的图片格式（仅支持 PNG / JPG / WEBP）".to_string()),
    };
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("读取图片失败：{e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(Some(format!("data:{mime};base64,{b64}")))
}

/// 读取未裁剪原图的 data URL（「重新裁剪」用）；不存在返回 None
#[tauri::command]
pub async fn read_background_original(state: State<'_, AppState>) -> Result<Option<String>, String> {
    use base64::Engine as _;
    for ext in BACKGROUND_EXTS {
        let path = state.paths.background_dir().join(format!("original.{ext}"));
        if let Ok(bytes) = tokio::fs::read(&path).await {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            return Ok(Some(format!("data:{};base64,{}", background_mime(ext), b64)));
        }
    }
    Ok(None)
}
