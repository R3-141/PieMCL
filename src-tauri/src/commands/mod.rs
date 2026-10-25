// Pie MCL 命令模块（tauri::command 入口统一挂载点）
//
// 目录约定：
// - instance: 实例隔离与版本管理（阶段二）
// - version:  MC 版本清单与加载器版本（阶段二）
// - java:      Java 探测与自动下载（后续）
// - download:  多线程下载引擎与资源补全（后续）
// - mods:      模组管理与在线安装（后续）
// - launch:    启动流程与进程管理（后续）
// - account:   账户系统（离线 + 微软正版）（后续）
//
// 约定：所有 command 返回 Result<T, String>，错误为中文可读文案。

pub mod account;
pub mod download;
pub mod game;
pub mod gameconfig;
pub mod instance;
pub mod java;
pub mod launch;
pub mod media;
pub mod mods;
pub mod natfrp;
pub mod pack;
pub mod servers;
pub mod settings;
pub mod version;
pub mod window;

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

use crate::core::paths::LauncherPaths;
use crate::core::settings::AppSettings;
use crate::download::model::DownloadRequest;
use crate::download::{DownloadEngine, ProgressCallback, TaskOptions};
use crate::instance::store::InstanceStore;
use crate::launch::process::RunningProcess;
use crate::version::repo::VersionRepo;

/// Tauri 全局状态：启动器路径 / 版本仓库 / 实例存储 / 下载引擎 / 全局设置
pub struct AppState {
    pub paths: LauncherPaths,
    pub versions: VersionRepo,
    pub instances: InstanceStore,
    /// 下载引擎（并发上限取自启动时的设置，修改并发数需重启生效）
    pub download: DownloadEngine,
    /// 全局设置（镜像列表修改实时生效）
    pub settings: Arc<RwLock<AppSettings>>,
    /// 下载互斥表：instance_id 或资源键（如 java:21）→ 运行中的 task_id
    /// （Arc 共享：启动流程的后台任务也要参与同一把互斥）
    pub active_downloads: Arc<StdMutex<HashMap<String, String>>>,
    /// 运行中的游戏进程：instance_id → 进程信息
    pub running: Arc<StdMutex<HashMap<String, Arc<RunningProcess>>>>,
    /// 被强制结束的实例：退出监视任务据此把退出码上报为 null（与崩溃区分）
    pub killed: Arc<StdMutex<std::collections::HashSet<String>>>,
    /// 失败/取消任务的下载定义（task_id → 定义），供 retry_download 重试
    pub retry_defs: Arc<StdMutex<HashMap<String, RetryDef>>>,
    /// 樱花内网穿透 frpc 进程槽（全局同时仅一个；None = 未运行）
    pub natfrp: Arc<StdMutex<Option<tokio::process::Child>>>,
}

/// 重试定义表（共享句柄；pub 供开发工具 bin 构造 ForgeInstallCtx）
pub type RetryMap = Arc<StdMutex<HashMap<String, RetryDef>>>;

/// 可重试任务的登记项
#[derive(Clone)]
pub struct RetryDef {
    pub opts: TaskOptions,
    pub requests: Vec<DownloadRequest>,
    /// 互斥锁键（完成后释放；None 表示未持锁）
    pub lock_key: Option<String>,
}

/// 登记可重试任务（start_task 前调用）
pub(crate) fn register_retry(
    map: &RetryMap,
    task_id: &str,
    opts: TaskOptions,
    requests: Vec<DownloadRequest>,
    lock_key: Option<String>,
) {
    map.lock().unwrap().insert(
        task_id.to_string(),
        RetryDef { opts, requests, lock_key },
    );
}

/// 移除重试登记（启动失败回滚 / completed 时）
pub(crate) fn remove_retry(map: &RetryMap, task_id: &str) {
    map.lock().unwrap().remove(task_id);
}

/// 构造带重试登记的进度回调：推 download-progress + 终态管理
/// （completed 移除重试定义；failed/cancelled 保留 + 释放互斥 + oneshot 可选通知）
pub(crate) fn make_retry_cb<R: tauri::Runtime>(
    retry_defs: RetryMap,
    active: Arc<StdMutex<HashMap<String, String>>>,
    app: AppHandle<R>,
    task_id: String,
    tx: Option<StdMutex<Option<tokio::sync::oneshot::Sender<Result<(), String>>>>>,
) -> ProgressCallback {
    use tauri::Emitter;
    Arc::new(move |p| {
        let _ = app.emit("download-progress", &p);
        if matches!(p.state.as_str(), "completed" | "failed" | "cancelled") {
            // 先取锁键再删登记：completed 会移除重试定义，若先删后查将拿不到
            // lock_key，导致互斥锁泄漏（成功路径的下一步下载全部撞锁）
            let lock_key = retry_defs
                .lock()
                .unwrap()
                .get(&task_id)
                .and_then(|d| d.lock_key.clone());
            if p.state == "completed" {
                retry_defs.lock().unwrap().remove(&task_id);
            }
            if let Some(key) = lock_key {
                active.lock().unwrap().remove(&key);
            }
            if let Some(s) = &tx {
                if let Some(s) = s.lock().unwrap().take() {
                    let _ = s.send(if p.state == "completed" {
                        Ok(())
                    } else {
                        Err(p.error.clone().unwrap_or_else(|| "下载失败".into()))
                    });
                }
            }
        }
    })
}

/// 构建进度回调：推送 download-progress 事件；
/// 终态（completed/failed/cancelled）释放互斥锁；
/// completed 移除重试定义、failed/cancelled 保留（供 retry_download）；
/// completed 时额外触发完成回调（如回写 loader.json / 解压 Java）
pub(crate) fn make_progress_callback(
    retry_defs: RetryMap,
    active_downloads: Arc<StdMutex<HashMap<String, String>>>,
    app: &AppHandle,
    lock_key: &str,
    on_completed: Option<Box<dyn Fn(AppHandle) + Send + Sync>>,
) -> ProgressCallback {
    let app = app.clone();
    let key = lock_key.to_string();
    Arc::new(move |p| {
        let _ = app.emit("download-progress", &p);
        if matches!(p.state.as_str(), "completed" | "failed" | "cancelled") {
            active_downloads.lock().unwrap().remove(&key);
            if p.state == "completed" {
                retry_defs.lock().unwrap().remove(&key);
            }
            if let Some(f) = &on_completed {
                f(app.clone());
            }
        }
    })
}

/// 启动下载任务：先取互斥锁（同实例 / 同资源互斥），登记可重试，启动失败时回滚。
/// 互斥表与下载引擎以参数传入：command 场景取自 AppState，启动流程场景取自 LaunchDeps。
pub(crate) async fn start_task(
    download: &DownloadEngine,
    active_downloads: Arc<StdMutex<HashMap<String, String>>>,
    retry_defs: RetryMap,
    app: &AppHandle,
    task_id: String,
    kind: &str,
    lock_key: &str,
    requests: Vec<DownloadRequest>,
    on_completed: Option<Box<dyn Fn(AppHandle) + Send + Sync>>,
) -> Result<(), String> {
    {
        let mut locks = active_downloads.lock().unwrap();
        if let Some(existing) = locks.get(lock_key) {
            return Err(format!("已有下载任务在进行中（{existing}），请等待完成或先取消"));
        }
        locks.insert(lock_key.to_string(), task_id.clone());
    }
    let opts = TaskOptions {
        task_id: task_id.clone(),
        kind: kind.to_string(),
        instance_id: Some(lock_key.to_string()),
    };
    register_retry(&retry_defs, &task_id, opts, requests.clone(), Some(lock_key.to_string()));
    let cb = make_progress_callback(
        retry_defs.clone(),
        active_downloads.clone(),
        app,
        lock_key,
        on_completed,
    );
    let result = download
        .start_task(
            TaskOptions {
                task_id: task_id.clone(),
                kind: kind.to_string(),
                instance_id: Some(lock_key.to_string()),
            },
            requests,
            cb,
        )
        .await;
    if result.is_err() {
        active_downloads.lock().unwrap().remove(lock_key);
        remove_retry(&retry_defs, &task_id);
    }
    result
}
