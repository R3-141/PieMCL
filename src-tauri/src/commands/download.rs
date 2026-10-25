// 下载任务控制命令：暂停 / 继续 / 取消
//
// task_id 来自 download-progress 进度事件，前端凭它控制对应任务。
// 进度本身由 Rust 侧经 download-progress 事件推送，前端零轮询。

use tauri::State;

use crate::commands::AppState;

/// 暂停下载任务：进行中的分块收尾落盘后停止（.part 与断点保留，可恢复）
#[tauri::command]
pub async fn pause_download(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    state.download.pause_task(&task_id).await
}

/// 恢复下载任务：从断点续传
#[tauri::command]
pub async fn resume_download(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    state.download.resume_task(&task_id).await
}

/// 取消下载任务：清理 .part 与断点文件
#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    state.download.cancel_task(&task_id).await
}

/// 重试失败 / 已取消的下载任务：以原 task_id 与原始请求重新启动
/// （进度事件覆盖原任务行；成功后重试定义自动移除）
#[tauri::command]
pub async fn retry_download(app: tauri::AppHandle, state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    let Some(def) = state.retry_defs.lock().unwrap().get(&task_id).cloned() else {
        return Err(format!("任务 {task_id} 不可重试（不存在或已成功）"));
    };
    // 重试期间重新占互斥锁（保留原锁键，防止与资源补全等并发）
    if let Some(key) = &def.lock_key {
        let mut locks = state.active_downloads.lock().unwrap();
        if let Some(existing) = locks.get(key) {
            return Err(format!("已有下载任务在进行中（{existing}），请稍后再试"));
        }
        locks.insert(key.clone(), task_id.clone());
    }
    let cb = super::make_retry_cb(
        state.retry_defs.clone(),
        state.active_downloads.clone(),
        app,
        task_id.clone(),
        None,
    );
    state
        .download
        .start_task(def.opts.clone(), def.requests.clone(), cb)
        .await
}
