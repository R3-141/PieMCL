// 版本 / 加载器元数据相关 Tauri commands

use tauri::State;

use crate::version::model::McVersionEntry;
use super::AppState;

/// 列出 MC 版本（show_snapshots 控制是否含快照版，refresh 强制刷新清单缓存）
#[tauri::command]
pub async fn list_mc_versions(
    state: State<'_, AppState>,
    show_snapshots: bool,
    refresh: bool,
) -> Result<Vec<McVersionEntry>, String> {
    state
        .versions
        .list_mc_versions(show_snapshots, refresh)
        .await
        .map_err(String::from)
}

/// 列出指定 MC 版本可用的加载器版本；vanilla 返回空列表
#[tauri::command]
pub async fn list_loader_versions(
    state: State<'_, AppState>,
    loader_type: String,
    mc_version: String,
) -> Result<Vec<String>, String> {
    state
        .versions
        .list_loader_versions(&loader_type, &mc_version)
        .await
        .map_err(String::from)
}
