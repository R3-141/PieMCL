// 截图与存档管理 commands（实例资源管理页「截图」「存档」选项卡）
//
// 截图目录 = {game_dir}/screenshots/*.png；存档目录 = {game_dir}/saves/<名>/
// （含 level.dat 的目录才算存档）。game_dir 由 InstanceStore::game_dir 解析
// （版本隔离开/关自动适配）。删除一律走回收站（trash），可恢复。
// 所有 name 参数做路径穿越校验。

use std::path::PathBuf;

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, State};

use super::AppState;

/// 文件名安全校验（防路径穿越）
fn safe_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
    {
        return Err("无效的文件名".into());
    }
    Ok(())
}

async fn game_dir_of(state: &State<'_, AppState>, instance_id: &str) -> Result<PathBuf, String> {
    state
        .instances
        .game_dir(instance_id)
        .await
        .map_err(String::from)
}

/// 文件条目（截图 / 存档共用）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub name: String,
    /// 字节数（存档为目录总大小）
    pub size: u64,
    /// 修改时间（Unix 毫秒）
    pub modified: u64,
    /// 存档是否带有自动生成的缩略图（saves/<名>/icon.png）
    pub has_icon: Option<bool>,
}

/// 目录递归收集文件（相对路径用 / 分隔，供 zip 打包）
fn collect_files(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_files(&p, base, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            out.push((p, rel));
        }
    }
}

fn metadata_item(name: String, p: &std::path::Path, dir: Option<&std::path::Path>) -> MediaItem {
    let meta = std::fs::metadata(p);
    let (size, modified) = match dir {
        // 目录总大小：递归累加
        Some(_) => {
            let mut files = Vec::new();
            collect_files(p, p, &mut files);
            let size = files
                .iter()
                .filter_map(|(f, _)| std::fs::metadata(f).ok())
                .map(|m| m.len())
                .sum();
            (size, meta.ok().and_then(|m| m.modified().ok()))
        }
        None => {
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            (size, meta.ok().and_then(|m| m.modified().ok()))
        }
    };
    MediaItem {
        name,
        size,
        modified: modified
            .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64)
            .unwrap_or(0),
        has_icon: None,
    }
}

// ---------------------------------------------------------------------------
// 截图
// ---------------------------------------------------------------------------

/// 截图列表（按修改时间倒序）
#[tauri::command]
pub async fn list_screenshots(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<MediaItem>, String> {
    let dir = game_dir_of(&state, &instance_id).await?.join("screenshots");
    tokio::task::spawn_blocking(move || {
        let mut out: Vec<MediaItem> = std::fs::read_dir(&dir)
            .map_err(|e| format!("读取截图目录失败：{e}"))?
            .flatten()
            .filter(|e| {
                let p = e.path();
                p.is_file()
                    && p.extension()
                        .and_then(|x| x.to_str())
                        .map(|x| x.eq_ignore_ascii_case("png"))
                        .unwrap_or(false)
            })
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                Some(metadata_item(name, &e.path(), None))
            })
            .collect();
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(out)
    })
    .await
    .map_err(|e| format!("扫描任务执行失败：{e}"))?
}

/// 读取截图为 data URL（前端缩略图 / 预览展示）
#[tauri::command]
pub async fn read_screenshot(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<String, String> {
    safe_name(&name)?;
    let path = game_dir_of(&state, &instance_id).await?.join("screenshots").join(&name);
    tokio::task::spawn_blocking(move || {
        let data = std::fs::read(&path).map_err(|e| format!("读取截图失败：{e}"))?;
        Ok(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(data)
        ))
    })
    .await
    .map_err(|e| format!("读取任务执行失败：{e}"))?
}

/// 删除截图（移入回收站）
#[tauri::command]
pub async fn delete_screenshot(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    safe_name(&name)?;
    let path = game_dir_of(&state, &instance_id).await?.join("screenshots").join(&name);
    tokio::task::spawn_blocking(move || trash::delete(&path).map_err(|e| format!("删除失败：{e}")))
        .await
        .map_err(|e| format!("删除任务执行失败：{e}"))?
}

/// 在资源管理器中打开截图所在位置并高亮
#[tauri::command]
pub async fn reveal_screenshot(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    safe_name(&name)?;
    let path = game_dir_of(&state, &instance_id).await?.join("screenshots").join(&name);
    if !path.is_file() {
        return Err("截图不存在".into());
    }
    tokio::task::spawn_blocking(move || opener::reveal(&path).map_err(|e| format!("打开文件位置失败：{e}")))
        .await
        .map_err(|e| format!("任务执行失败：{e}"))?
}

/// 复制截图到系统剪贴板（位图形式，可在画图 / 聊天窗口直接粘贴）
#[tauri::command]
pub async fn copy_screenshot_to_clipboard(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    safe_name(&name)?;
    let path = game_dir_of(&state, &instance_id).await?.join("screenshots").join(&name);
    tokio::task::spawn_blocking(move || {
        let img = image::ImageReader::open(&path)
            .map_err(|e| format!("读取截图失败：{e}"))?
            .decode()
            .map_err(|e| format!("解码截图失败：{e}"))?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        let mut clip = arboard::Clipboard::new().map_err(|e| format!("打开剪贴板失败：{e}"))?;
        clip.set_image(arboard::ImageData {
            width: w,
            height: h,
            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
        })
        .map_err(|e| format!("写入剪贴板失败：{e}"))
    })
    .await
    .map_err(|e| format!("任务执行失败：{e}"))?
}

// ---------------------------------------------------------------------------
// 存档
// ---------------------------------------------------------------------------

/// 存档列表（按修改时间倒序；has_icon 标记是否有自动生成的 icon.png 缩略图）
#[tauri::command]
pub async fn list_saves(state: State<'_, AppState>, instance_id: String) -> Result<Vec<MediaItem>, String> {
    let dir = game_dir_of(&state, &instance_id).await?.join("saves");
    tokio::task::spawn_blocking(move || {
        let mut out: Vec<MediaItem> = std::fs::read_dir(&dir)
            .map_err(|e| format!("读取存档目录失败：{e}"))?
            .flatten()
            .filter(|e| e.path().is_dir() && e.path().join("level.dat").is_file())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let has_icon = e.path().join("icon.png").is_file();
                let mut item = metadata_item(name, &e.path(), Some(&e.path()));
                item.has_icon = Some(has_icon);
                Some(item)
            })
            .collect();
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(out)
    })
    .await
    .map_err(|e| format!("扫描任务执行失败：{e}"))?
}

/// 读取存档缩略图（MC 自动生成的 saves/<名>/icon.png）data URL；无缩略图返回 None
#[tauri::command]
pub async fn read_save_icon(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<Option<String>, String> {
    safe_name(&name)?;
    let path = game_dir_of(&state, &instance_id)
        .await?
        .join("saves")
        .join(&name)
        .join("icon.png");
    tokio::task::spawn_blocking(move || {
        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => return Ok(None),
        };
        Ok(Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(data)
        )))
    })
    .await
    .map_err(|e| format!("读取任务执行失败：{e}"))?
}

/// 打开实例的存档目录（资源管理器）
#[tauri::command]
pub async fn open_saves_dir(state: State<'_, AppState>, instance_id: String) -> Result<(), String> {
    let dir = game_dir_of(&state, &instance_id).await?.join("saves");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("创建存档目录失败：{e}"))?;
    opener::open(&dir).map_err(|e| format!("打开存档目录失败：{e}"))
}

/// 删除存档（整个目录移入回收站）
#[tauri::command]
pub async fn delete_save(
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    safe_name(&name)?;
    let path = game_dir_of(&state, &instance_id).await?.join("saves").join(&name);
    if !path.is_dir() {
        return Err("存档不存在".into());
    }
    tokio::task::spawn_blocking(move || trash::delete(&path).map_err(|e| format!("删除失败：{e}")))
        .await
        .map_err(|e| format!("删除任务执行失败：{e}"))?
}

/// 备份存档：弹系统保存对话框选位置 → zip 打包整个存档目录，返回保存路径
#[tauri::command]
pub async fn backup_save(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    name: String,
) -> Result<String, String> {
    safe_name(&name)?;
    let src = game_dir_of(&state, &instance_id).await?.join("saves").join(&name);
    if !src.is_dir() {
        return Err("存档不存在".into());
    }
    use tauri_plugin_dialog::DialogExt;
    let default_name = format!(
        "{name}-{}.zip",
        chrono_like_date()
    );
    let app2 = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .add_filter("ZIP 压缩包", &["zip"])
            .set_file_name(&default_name)
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("对话框任务执行失败：{e}"))?;
    let Some(picked) = picked else {
        return Err("已取消备份".into());
    };
    let dest = picked.into_path().map_err(|e| format!("获取路径失败：{e}"))?;

    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::create(&dest).map_err(|e| format!("创建备份文件失败：{e}"))?;
        let mut zip = zip::ZipWriter::new(file);
        let options =
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut files = Vec::new();
        collect_files(&src, &src, &mut files);
        files.sort_by(|a, b| a.1.cmp(&b.1));
        for (path, rel) in &files {
            zip.start_file(rel.clone(), options)
                .map_err(|e| format!("写入压缩包失败：{e}"))?;
            let data = std::fs::read(path).map_err(|e| format!("读取文件失败：{e}"))?;
            std::io::Write::write_all(&mut zip, &data).map_err(|e| format!("写入压缩包失败：{e}"))?;
        }
        zip.finish().map_err(|e| format!("完成压缩包失败：{e}"))?;
        Ok(dest.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("备份任务执行失败：{e}"))?
}

/// 简单日期串（YYYYMMDD），用于默认备份文件名
fn chrono_like_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    // 本地时区按 +8 偏移（项目运行环境 Asia/Shanghai）
    let days = now.div_euclid(86400);
    let secs = now.rem_euclid(86400);
    // 平铺日期计算（自 1970-01-01 起）
    let (y, m, d) = civil_from_days(days);
    let hh = secs / 3600;
    let mm = (secs % 3600) / 60;
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}")
}

/// 天数 → 公历日期（Howard Hinnant 算法）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
