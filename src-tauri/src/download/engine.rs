// 下载引擎：并发调度、分块下载、断点续传、哈希校验与原子写入
//
// 设计要点（阶段三计划已确认）：
// - 并发：全局 Semaphore（默认 64 连接）+ 每主机 Semaphore（默认 16），
//   固定"先全局、后主机"的加锁顺序，避免交叉等待死锁
// - 分块：≥16MB 且服务器支持 Range 的文件最多分 8 块并发下载；其余走单流
// - 断点：{tmp}/{key}.part + {key}.meta.json 分块位图；
//   暂停时保留（可恢复），取消 / 最终失败 / 完成时清理
// - 重试：指数退避 1s/2s/4s，最多重试 3 次；每次重试轮换 URL 候选
//   （调用方按"镜像优先、官方兜底"编排候选列表），换源后旧断点不可信、清空重下
// - 原子写：数据只写 .part，流式哈希校验通过后 rename 到目标路径，
//   从不直接向目标文件写入（校验失败也不会污染已有文件）
// - 进度：原子计数器 + 200ms 节流聚合 + 文件完成即时推送（download-progress 事件），
//   前端零轮询

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use reqwest::header::{self, HeaderMap};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::{watch, Mutex as AsyncMutex, Notify, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout, MissedTickBehavior};

use super::hash;
use super::model::{DownloadProgress, DownloadRequest};
use crate::core::error::{AppError, AppResult};

/// 分块阈值：≥8MB 且服务器支持 Range 才分块，小文件靠文件级并发
const CHUNK_THRESHOLD: u64 = 8 * 1024 * 1024;
/// 最大分块数
const MAX_CHUNKS: u64 = 16;
/// 最大重试次数（指数退避 1/2/4 秒，共 1 + 3 次尝试）
const MAX_RETRIES: u32 = 3;
/// 单次读取超时：超过视为连接停滞，触发重试续传
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// 进度推送节流间隔
const EMIT_INTERVAL: Duration = Duration::from_millis(200);
/// 即时推送（文件完成）的最小间隔，避免海量小文件刷爆事件通道
const MIN_EMIT_GAP: Duration = Duration::from_millis(100);
/// 断点位图落盘阈值：每流累计写入 ≥1MB 刷一次 meta，降低磁盘压力
const META_FLUSH_BYTES: u64 = 1024 * 1024;

// 任务状态（watch 通道内的 u8 值）
const ST_RUNNING: u8 = 0;
const ST_PAUSED: u8 = 1;
const ST_CANCELLED: u8 = 2;

/// 进度回调：command 层用 AppHandle 包装为 Tauri 事件推送
pub type ProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync + 'static>;

/// 任务描述信息
#[derive(Debug, Clone)]
pub struct TaskOptions {
    /// 任务唯一 ID（进度事件原样带回，前端凭它暂停/继续/取消）
    pub task_id: String,
    /// 任务类型：resources / loader / java
    pub kind: String,
    /// 关联实例（无则 None）
    pub instance_id: Option<String>,
}

// ---------------------------------------------------------------------------
// 任务控制与进度快照
// ---------------------------------------------------------------------------

/// 任务运行控制（watch 通道驱动，无轮询）
#[derive(Clone)]
struct TaskControl {
    tx: watch::Sender<u8>,
    rx: watch::Receiver<u8>,
}

impl TaskControl {
    fn new() -> Self {
        let (tx, rx) = watch::channel(ST_RUNNING);
        Self { tx, rx }
    }

    fn state(&self) -> u8 {
        *self.rx.borrow()
    }

    fn pause(&self) {
        self.tx.send_if_modified(|s| {
            if *s == ST_RUNNING {
                *s = ST_PAUSED;
                true
            } else {
                false
            }
        });
    }

    fn resume(&self) {
        self.tx.send_if_modified(|s| {
            if *s == ST_PAUSED {
                *s = ST_RUNNING;
                true
            } else {
                false
            }
        });
    }

    fn cancel(&self) {
        self.tx.send_if_modified(|s| {
            if *s != ST_CANCELLED {
                *s = ST_CANCELLED;
                true
            } else {
                false
            }
        });
    }

    /// 等待离开暂停态（恢复或取消都会唤醒）
    async fn wait_unpaused(&self) {
        let mut rx = self.rx.clone();
        loop {
            if *rx.borrow() != ST_PAUSED {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }
}

/// 任务级进度计数（原子变量，多 worker 并发更新）
struct TaskProgress {
    completed_files: AtomicU64,
    downloaded_bytes: AtomicU64,
    total_files: AtomicU64,
    total_bytes: AtomicU64,
    current_file: StdMutex<Option<String>>,
    /// 文件完成信号：唤醒推送任务做即时上报
    file_done: Notify,
}

impl TaskProgress {
    fn set_current(&self, key: &str) {
        *self.current_file.lock().unwrap() = Some(key.to_string());
    }
}

// ---------------------------------------------------------------------------
// 断点位图（.meta.json）
// ---------------------------------------------------------------------------

/// 单个分块的下载进度
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkMeta {
    /// 起始字节（含）
    start: u64,
    /// 末尾字节（含）；None = 总长未知（单流追加模式，仅支持同源续传）
    end: Option<u64>,
    /// 该块已落盘字节数
    done: u64,
}

impl ChunkMeta {
    fn len(&self) -> Option<u64> {
        self.end.map(|e| e + 1 - self.start)
    }

    fn complete(&self) -> bool {
        self.len().is_none_or(|l| self.done >= l)
    }
}

/// 断点位图文件内容
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PartMeta {
    /// 断点对应的源 URL：换源（重试轮换）后断点不可信，需清空重下
    url: String,
    total: Option<u64>,
    chunks: Vec<ChunkMeta>,
}

/// 断点内存态：多分块 worker 共享，落盘按字节阈值节流
struct PartState {
    meta: StdMutex<PartMeta>,
    meta_path: PathBuf,
    pending: AtomicU64,
}

impl PartState {
    fn new_arc(meta: PartMeta, meta_path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            meta: StdMutex::new(meta),
            meta_path,
            pending: AtomicU64::new(0),
        })
    }

    fn chunk(&self, idx: usize) -> ChunkMeta {
        self.meta.lock().unwrap().chunks[idx].clone()
    }

    fn set_done(&self, idx: usize, done: u64) {
        self.meta.lock().unwrap().chunks[idx].done = done;
    }

    fn total(&self) -> Option<u64> {
        self.meta.lock().unwrap().total
    }

    fn record_bytes(&self, n: u64) {
        self.pending.fetch_add(n, Ordering::Relaxed);
    }

    fn should_flush(&self) -> bool {
        self.pending.load(Ordering::Relaxed) >= META_FLUSH_BYTES
    }

    /// 落盘断点位图（失败不致命：断点落后最多 1MB，续传时按实际覆盖重下）
    async fn flush(&self) {
        let text = {
            let m = self.meta.lock().unwrap();
            match serde_json::to_string(&*m) {
                Ok(t) => t,
                Err(_) => return,
            }
        };
        let _ = tokio::fs::write(&self.meta_path, text).await;
        self.pending.store(0, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// 引擎本体
// ---------------------------------------------------------------------------

/// 每主机信号量集合 + 连接许可
struct EngineInner {
    client: reqwest::Client,
    global: Arc<Semaphore>,
    per_host: usize,
    hosts: AsyncMutex<HashMap<String, Arc<Semaphore>>>,
    tmp_dir: PathBuf,
    /// 分块阈值（测试可调小）
    chunk_threshold: u64,
    /// 退避秒数（测试可置 0）
    backoff_secs: [u64; 3],
    /// 跨任务 per-dest 锁：assets/libraries 全局共享后，多个实例可能并发补全
    /// 同一文件 —— 持锁串行化后，后者在拿到锁时发现文件已存在即跳过
    dest_locks: AsyncMutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>,
}

impl EngineInner {
    /// 获取目标路径的跨任务锁（懒创建，锁对象随任务存活被复用后自动回收不必要）
    async fn dest_lock(&self, dest: &Path) -> Arc<AsyncMutex<()>> {
        let mut map = self.dest_locks.lock().await;
        map.entry(dest.to_path_buf())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}

/// 一次 HTTP 请求占用的连接许可（RAII，drop 即释放）
struct Permits {
    _global: OwnedSemaphorePermit,
    _host: OwnedSemaphorePermit,
}

impl EngineInner {
    fn new(tmp_dir: PathBuf, global: usize, per_host: usize) -> Self {
        let client = crate::core::net::apply_proxy(reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .user_agent("PieMCL/0.1"))
            .build()
            .expect("初始化 HTTP 客户端失败");
        Self {
            client,
            global: Arc::new(Semaphore::new(global.max(1))),
            per_host: per_host.max(1),
            hosts: AsyncMutex::new(HashMap::new()),
            tmp_dir,
            chunk_threshold: CHUNK_THRESHOLD,
            backoff_secs: [1, 2, 4],
            dest_locks: AsyncMutex::new(HashMap::new()),
        }
    }

    /// 获取连接许可：先全局后主机（固定顺序避免死锁）。
    /// per_host_override 非空时使用独立的（更小的）每主机信号量——同一主机
    /// 不同并发档位各自计数，互不挤占
    async fn acquire_for(&self, url: &str, per_host_override: Option<usize>) -> AppResult<Permits> {
        let host = Url::parse(url)
            .map_err(|e| AppError::msg(format!("URL 无效：{e}")))?
            .host_str()
            .unwrap_or("unknown")
            .to_string();
        let limit = per_host_override.unwrap_or(self.per_host);
        // 带覆盖档位的主机单独建信号量（键加后缀），避免与默认档位混用计数
        let host_key = match per_host_override {
            Some(n) => format!("{host}#{n}"),
            None => host,
        };
        let host_sem = {
            let mut map = self.hosts.lock().await;
            map.entry(host_key)
                .or_insert_with(|| Arc::new(Semaphore::new(limit)))
                .clone()
        };
        let g = self
            .global
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AppError::msg("下载引擎已关闭"))?;
        let h = host_sem
            .acquire_owned()
            .await
            .map_err(|_| AppError::msg("下载引擎已关闭"))?;
        Ok(Permits { _global: g, _host: h })
    }

    /// 断点文件路径：以 task_id + 目标路径哈希为键，
    /// 不同任务各用各的临时文件，天然无冲突
    fn tmp_paths(&self, task_id: &str, dest: &Path) -> (PathBuf, PathBuf) {
        let mut h = Sha256::new();
        Digest::update(&mut h, format!("{task_id}:{}", dest.to_string_lossy()).as_bytes());
        let key = &hex::encode(Digest::finalize(h))[..16];
        (
            self.tmp_dir.join(format!("{key}.part")),
            self.tmp_dir.join(format!("{key}.meta.json")),
        )
    }
}

/// 正在运行的任务登记项（供暂停/继续/取消查询）
struct TaskEntry {
    ctrl: TaskControl,
}

/// 下载引擎：一个启动器进程共享一个实例
#[derive(Clone)]
pub struct DownloadEngine {
    inner: Arc<EngineInner>,
    tasks: Arc<AsyncMutex<HashMap<String, TaskEntry>>>,
}

impl DownloadEngine {
    pub fn new(tmp_dir: PathBuf, global_concurrency: usize, per_host_concurrency: usize) -> Self {
        Self {
            inner: Arc::new(EngineInner::new(tmp_dir, global_concurrency, per_host_concurrency)),
            tasks: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn new_for_test(
        tmp_dir: PathBuf,
        global: usize,
        per_host: usize,
        chunk_threshold: u64,
        backoff_secs: [u64; 3],
) -> Self {
        let mut e = Self::new(tmp_dir, global, per_host);
        let inner = Arc::get_mut(&mut e.inner).unwrap();
        inner.chunk_threshold = chunk_threshold;
        inner.backoff_secs = backoff_secs;
        e
    }

    /// 启动一个下载任务：立即返回，进度经回调异步推送
    pub async fn start_task(
        &self,
        opts: TaskOptions,
        requests: Vec<DownloadRequest>,
        on_progress: ProgressCallback,
    ) -> Result<(), String> {
        if requests.is_empty() {
            return Err("下载任务没有任何文件".into());
        }
        if let Some(r) = requests.iter().find(|r| r.urls.is_empty()) {
            return Err(format!("下载请求「{}」没有可用的 URL", r.key));
        }
        // 同一目标路径只保留一个请求：并发写同一 .part 会在 rename 收尾时
        // 互相抢文件（os error 2）。列表来源（如 install_profile.libraries）
        // 可能含重复条目，这里统一去重。
        let mut seen = std::collections::HashSet::new();
        let requests: Vec<DownloadRequest> = requests
            .into_iter()
            .filter(|r| seen.insert(r.dest.clone()))
            .collect();
        if requests.is_empty() {
            return Err("下载任务没有任何文件".into());
        }

        let mut tasks = self.tasks.lock().await;
        if tasks.contains_key(&opts.task_id) {
            return Err(format!("下载任务已在运行：{}", opts.task_id));
        }
        let ctrl = TaskControl::new();
        let prog = Arc::new(TaskProgress {
            completed_files: AtomicU64::new(0),
            downloaded_bytes: AtomicU64::new(0),
            total_files: AtomicU64::new(requests.len() as u64),
            total_bytes: AtomicU64::new(requests.iter().filter_map(|r| r.size).sum::<u64>()),
            current_file: StdMutex::new(None),
            file_done: Notify::new(),
        });
        let (fin_tx, _) = watch::channel(false);
        tasks.insert(opts.task_id.clone(), TaskEntry { ctrl: ctrl.clone() });
        drop(tasks);

        let emit = Arc::new(EmitCore {
            cb: on_progress,
            task_id: opts.task_id.clone(),
            kind: opts.kind,
            instance_id: opts.instance_id,
            prog: prog.clone(),
            state_rx: ctrl.rx.clone(),
            inner: StdMutex::new(EmitState {
                finished: false,
                last_emit: None,
                last_bytes: 0,
                last_bps: 0.0,
            }),
        });

        let inner = self.inner.clone();
        let tasks_map = self.tasks.clone();
        let task_id = opts.task_id.clone();
        // 整合包任务（大量小文件 + mcimirror 镜像源）：压低每主机并发，
        // 避免齐发连接触发镜像限流反而更慢（实测 8 明显优于 64）；其余任务不变
        let per_host = if emit.kind == "pack" { Some(8) } else { None };
        tokio::spawn(supervisor(
            inner,
            ctrl,
            prog,
            emit,
            fin_tx,
            requests,
            task_id,
            tasks_map,
            per_host,
        ));
        Ok(())
    }

    pub async fn pause_task(&self, task_id: &str) -> Result<(), String> {
        let tasks = self.tasks.lock().await;
        match tasks.get(task_id) {
            Some(e) => {
                e.ctrl.pause();
                Ok(())
            }
            None => Err(format!("下载任务不存在或已结束：{task_id}")),
        }
    }

    pub async fn resume_task(&self, task_id: &str) -> Result<(), String> {
        let tasks = self.tasks.lock().await;
        match tasks.get(task_id) {
            Some(e) => {
                e.ctrl.resume();
                Ok(())
            }
            None => Err(format!("下载任务不存在或已结束：{task_id}")),
        }
    }

    pub async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        let tasks = self.tasks.lock().await;
        match tasks.get(task_id) {
            Some(e) => {
                e.ctrl.cancel();
                Ok(())
            }
            None => Err(format!("下载任务不存在或已结束：{task_id}")),
        }
    }
}

// ---------------------------------------------------------------------------
// 任务监督循环
// ---------------------------------------------------------------------------

/// 单文件 worker 的结果
#[derive(Debug)]
enum FileOutcome {
    Completed,
    Skipped,
    Paused,
    Cancelled,
    Failed(String),
}

/// 传给文件 worker 的任务上下文
#[derive(Clone)]
struct TaskCtx {
    inner: Arc<EngineInner>,
    ctrl: TaskControl,
    prog: Arc<TaskProgress>,
    task_id: String,
    done: Arc<Vec<AtomicBool>>,
    idx: usize,
    /// 每主机并发覆盖（None = 引擎默认）；整合包任务压低并发避开镜像限流
    per_host: Option<usize>,
}

/// 任务监督循环：按 pass 反复调度未完成文件，处理暂停/取消/失败终态
#[allow(clippy::too_many_arguments)]
async fn supervisor(
    inner: Arc<EngineInner>,
    ctrl: TaskControl,
    prog: Arc<TaskProgress>,
    emit: Arc<EmitCore>,
    fin_tx: watch::Sender<bool>,
    requests: Vec<DownloadRequest>,
    task_id: String,
    tasks_map: Arc<AsyncMutex<HashMap<String, TaskEntry>>>,
    per_host: Option<usize>,
) {
    // 进度推送节拍器：200ms 聚合 + 文件完成即时推
    let ticker = {
        let emit = emit.clone();
        let prog = prog.clone();
        let mut fin = fin_tx.subscribe();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(EMIT_INTERVAL);
            tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = tick.tick() => emit.emit(EmitKind::Running { force: false }),
                    _ = prog.file_done.notified() => emit.emit(EmitKind::Running { force: true }),
                    r = fin.changed() => {
                        if r.is_err() || *fin.borrow_and_update() {
                            break;
                        }
                    }
                }
            }
        })
    };

    let done: Arc<Vec<AtomicBool>> =
        Arc::new((0..requests.len()).map(|_| AtomicBool::new(false)).collect());

    // 初始事件：前端立即拿到 task_id 与总量
    emit.emit(EmitKind::Running { force: true });

    loop {
        // 本轮：为所有未完成文件派发 worker
        let mut set: JoinSet<FileOutcome> = JoinSet::new();
        for (i, req) in requests.iter().enumerate() {
            if done[i].load(Ordering::Relaxed) {
                continue;
            }
            let ctx = TaskCtx {
                inner: inner.clone(),
                ctrl: ctrl.clone(),
                prog: prog.clone(),
                task_id: task_id.clone(),
                done: done.clone(),
                idx: i,
                per_host,
            };
            set.spawn(download_file(ctx, req.clone()));
        }

        let mut paused = false;
        let mut cancelled = false;
        let mut failure: Option<String> = None;
        while let Some(res) = set.join_next().await {
            let outcome = match res {
                Ok(o) => o,
                Err(e) if e.is_cancelled() => continue,
                Err(_) => FileOutcome::Failed("内部下载协程异常退出".into()),
            };
            match outcome {
                FileOutcome::Failed(msg) => {
                    if failure.is_none() {
                        failure = Some(msg);
                        set.abort_all();
                    }
                }
                FileOutcome::Paused => paused = true,
                FileOutcome::Cancelled => cancelled = true,
                _ => {}
            }
        }

        let state = ctrl.state();
        if cancelled || state == ST_CANCELLED {
            cleanup_parts(&inner, &task_id, &requests, &done).await;
            emit.emit(EmitKind::Terminal("cancelled", None));
            break;
        }
        if let Some(msg) = failure {
            cleanup_parts(&inner, &task_id, &requests, &done).await;
            emit.emit(EmitKind::Terminal("failed", Some(msg)));
            break;
        }
        if paused || state == ST_PAUSED {
            // 暂停语义：进行中的分块已收尾、断点已落盘；等待恢复或取消
            emit.emit(EmitKind::Paused);
            ctrl.wait_unpaused().await;
            if ctrl.state() == ST_CANCELLED {
                cleanup_parts(&inner, &task_id, &requests, &done).await;
                emit.emit(EmitKind::Terminal("cancelled", None));
                break;
            }
            continue;
        }
        // 所有文件完成
        emit.emit(EmitKind::Terminal("completed", None));
        break;
    }

    let _ = fin_tx.send(true);
    let _ = ticker.await;
    tasks_map.lock().await.remove(&task_id);
}

/// 清理未完成文件的 .part 与断点位图（取消 / 最终失败时）
async fn cleanup_parts(
    inner: &EngineInner,
    task_id: &str,
    requests: &[DownloadRequest],
    done: &[AtomicBool],
) {
    for (i, req) in requests.iter().enumerate() {
        if done[i].load(Ordering::Relaxed) {
            continue;
        }
        let (part, meta) = inner.tmp_paths(task_id, &req.dest);
        let _ = tokio::fs::remove_file(&part).await;
        let _ = tokio::fs::remove_file(&meta).await;
    }
}

// ---------------------------------------------------------------------------
// 单文件下载
// ---------------------------------------------------------------------------

/// worker 内部错误
#[derive(Debug)]
enum ChunkErr {
    Failed(AppError),
    Paused,
    Cancelled,
    /// 服务器对分块请求返回 200（忽略 Range），需回退单流
    RangeUnsupported,
}

impl From<AppError> for ChunkErr {
    fn from(e: AppError) -> Self {
        ChunkErr::Failed(e)
    }
}

impl From<reqwest::Error> for ChunkErr {
    fn from(e: reqwest::Error) -> Self {
        ChunkErr::Failed(e.into())
    }
}

impl From<std::io::Error> for ChunkErr {
    fn from(e: std::io::Error) -> Self {
        ChunkErr::Failed(AppError::Io(e))
    }
}

/// 回退本文件已计入进度的字节数（断点被清空时调用，避免进度虚高）
fn rollback(prog: &TaskProgress, written: &AtomicU64) {
    let w = written.swap(0, Ordering::Relaxed);
    if w > 0 {
        prog.downloaded_bytes.fetch_sub(w, Ordering::Relaxed);
    }
}

/// 部分回退进度：仅回退本任务实际计入过的字节
/// （外部预置断点的 done 可能大于本任务计入量，不能多扣导致计数下溢）
fn subtract_progress(prog: &TaskProgress, written: &AtomicU64, amount: u64) {
    loop {
        let w = written.load(Ordering::Relaxed);
        let sub = w.min(amount);
        if sub == 0 {
            return;
        }
        if written.compare_exchange_weak(w, w - sub, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            prog.downloaded_bytes.fetch_sub(sub, Ordering::Relaxed);
            return;
        }
    }
}

async fn wipe_partial(part: &Path, meta: &Path) {
    let _ = tokio::fs::remove_file(part).await;
    let _ = tokio::fs::remove_file(meta).await;
}

/// 等待任务被取消（用于让退避等待期间的取消请求立即生效）
async fn wait_cancelled(ctrl: &TaskControl) {
    let mut rx = ctrl.rx.clone();
    loop {
        if *rx.borrow_and_update() == ST_CANCELLED {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

/// 读取断点位图；仅当 URL 一致时可信
async fn load_resume_meta(meta_path: &Path, url: &str) -> Option<PartMeta> {
    let text = tokio::fs::read_to_string(meta_path).await.ok()?;
    let meta: PartMeta = serde_json::from_str(&text).ok()?;
    if meta.url != url || meta.chunks.is_empty() {
        return None;
    }
    Some(meta)
}

/// 下载单个文件：先取跨任务 per-dest 锁（等待期间取消立即生效），
/// 再进入实际下载 —— 后到者在拿到锁时发现文件已就绪即跳过
async fn download_file(ctx: TaskCtx, req: DownloadRequest) -> FileOutcome {
    let lock = ctx.inner.dest_lock(&req.dest).await;
    let _guard = tokio::select! {
        g = lock.lock() => g,
        _ = wait_cancelled(&ctx.ctrl) => return FileOutcome::Cancelled,
    };
    download_file_locked(ctx, req).await
}

/// 下载单个文件（含增量跳过、重试退避、URL 轮换、断点续传）；调用方须持有 dest 锁
async fn download_file_locked(ctx: TaskCtx, req: DownloadRequest) -> FileOutcome {
    // 1) 本地已有：哈希匹配（或无期望哈希）即视为完成 —— 只下载缺失或损坏部分
    if let Ok(md) = tokio::fs::metadata(&req.dest).await {
        let ok = match &req.expected {
            Some((algo, hex_str)) => hash::verify_file(&req.dest, *algo, hex_str).await.unwrap_or(false),
            None => true,
        };
        if ok {
            ctx.prog.downloaded_bytes.fetch_add(md.len(), Ordering::Relaxed);
            ctx.prog.completed_files.fetch_add(1, Ordering::Relaxed);
            ctx.prog.file_done.notify_one();
            ctx.done[ctx.idx].store(true, Ordering::Relaxed);
            return FileOutcome::Skipped;
        }
    }

    ctx.prog.set_current(&req.key);
    let (part_path, meta_path) = ctx.inner.tmp_paths(&ctx.task_id, &req.dest);
    // 本文件本次任务已计入进度的字节数（断点清空时回退）
    let written = Arc::new(AtomicU64::new(0));
    let size_added = AtomicBool::new(false);

    let mut last_err = String::new();
    for attempt in 0..=(MAX_RETRIES as usize) {
        if attempt > 0 {
            let secs = ctx.inner.backoff_secs[attempt - 1];
            if secs > 0 {
                // 退避等待期间取消立即生效（否则点取消最多要等 4 秒）
                tokio::select! {
                    _ = sleep(Duration::from_secs(secs)) => {}
                    _ = wait_cancelled(&ctx.ctrl) => return FileOutcome::Cancelled,
                }
            }
        }
        if ctx.ctrl.state() == ST_CANCELLED {
            return FileOutcome::Cancelled;
        }
        // 重试轮换 URL 候选：镜像优先、官方兜底
        let url = req.urls[attempt % req.urls.len()].clone();
        // 换源后旧断点不可信：清空重下
        let resume = load_resume_meta(&meta_path, &url).await;
        if resume.is_none() {
            wipe_partial(&part_path, &meta_path).await;
            rollback(&ctx.prog, &written);
        }

        let outcome =
            attempt_once(&ctx, &req, &url, &part_path, &meta_path, resume, &written, &size_added)
                .await;
        match outcome {
            Ok(()) => {
                ctx.done[ctx.idx].store(true, Ordering::Relaxed);
                ctx.prog.completed_files.fetch_add(1, Ordering::Relaxed);
                ctx.prog.file_done.notify_one();
                return FileOutcome::Completed;
            }
            Err(ChunkErr::Paused) => return FileOutcome::Paused,
            Err(ChunkErr::Cancelled) => {
                wipe_partial(&part_path, &meta_path).await;
                rollback(&ctx.prog, &written);
                return FileOutcome::Cancelled;
            }
            Err(ChunkErr::Failed(e)) => last_err = e.to_string(),
            Err(ChunkErr::RangeUnsupported) => {
                last_err = "服务器不支持断点续传下载".into();
            }
        }
    }

    // 重试耗尽：清断点，报失败
    wipe_partial(&part_path, &meta_path).await;
    rollback(&ctx.prog, &written);
    FileOutcome::Failed(format!("{}：{}", req.key, last_err))
}

/// 一次完整尝试：分块或单流；分块遇 200 自动回退单流
#[allow(clippy::too_many_arguments)]
async fn attempt_once(
    ctx: &TaskCtx,
    req: &DownloadRequest,
    url: &str,
    part_path: &Path,
    meta_path: &Path,
    resume: Option<PartMeta>,
    written: &Arc<AtomicU64>,
    size_added: &AtomicBool,
) -> Result<(), ChunkErr> {
    let mut resume = resume;
    let mut force_single = false;
    loop {
        let r = if let Some(meta) = resume.take() {
            try_resume(ctx, req, url, part_path, meta_path, meta, written, size_added).await
        } else {
            try_fresh(ctx, req, url, part_path, meta_path, force_single, written, size_added).await
        };
        match r {
            Err(ChunkErr::RangeUnsupported) => {
                // 服务器忽略 Range：清断点，单流重试一次
                wipe_partial(part_path, meta_path).await;
                rollback(&ctx.prog, written);
                force_single = true;
                continue;
            }
            other => return other,
        }
    }
}

/// 从已有断点继续
async fn try_resume(
    ctx: &TaskCtx,
    req: &DownloadRequest,
    url: &str,
    part_path: &Path,
    meta_path: &Path,
    meta: PartMeta,
    written: &Arc<AtomicU64>,
    size_added: &AtomicBool,
) -> Result<(), ChunkErr> {
    let state = PartState::new_arc(meta, meta_path.to_path_buf());
    let multi = state.meta.lock().unwrap().chunks.len() > 1;
    if !multi {
        return run_single_resume(ctx, req, url, part_path, meta_path, state, written).await;
    }

    // 分块续传：校验 .part 与总长一致，不一致则断点文件损坏、清空重来
    if let Some(t) = state.total() {
        let len = tokio::fs::metadata(part_path).await.map(|m| m.len()).unwrap_or(0);
        if len != t {
            wipe_partial(part_path, meta_path).await;
            rollback(&ctx.prog, written);
            return try_fresh(ctx, req, url, part_path, meta_path, false, written, size_added).await;
        }
    }
    run_chunked(ctx, url, part_path, state, written).await?;
    finalize_file(req, part_path, meta_path).await.map_err(ChunkErr::Failed)
}

/// 全新下载：探测 Range 支持与总长，决定分块或单流
async fn try_fresh(
    ctx: &TaskCtx,
    req: &DownloadRequest,
    url: &str,
    part_path: &Path,
    meta_path: &Path,
    force_single: bool,
    written: &Arc<AtomicU64>,
    size_added: &AtomicBool,
) -> Result<(), ChunkErr> {
    let _permits = ctx.inner.acquire_for(url, ctx.per_host).await?;
    match ctx.ctrl.state() {
        ST_CANCELLED => return Err(ChunkErr::Cancelled),
        ST_PAUSED => return Err(ChunkErr::Paused),
        _ => {}
    }

    let mut builder = ctx.inner.client.get(url);
    if !force_single {
        builder = builder.header(header::RANGE, "bytes=0-");
    }
    let resp = builder.send().await?;
    let status = resp.status();

    let (range_ok, total) = if status == StatusCode::PARTIAL_CONTENT {
        (true, parse_total(resp.headers()))
    } else if status == StatusCode::OK {
        (false, resp.content_length())
    } else {
        return Err(ChunkErr::Failed(AppError::msg(format!("服务器返回异常状态 {status}"))));
    };

    // 请求方未知大小时，用服务器告知的真实总长补全进度总量（每文件只补一次）
    if let Some(t) = total {
        if req.size.is_none() && !size_added.swap(true, Ordering::Relaxed) {
            ctx.prog.total_bytes.fetch_add(t, Ordering::Relaxed);
        }
    }

    let do_chunk =
        range_ok && !force_single && total.map(|t| t >= ctx.inner.chunk_threshold).unwrap_or(false);

    if do_chunk {
        let t = total.unwrap();
        drop(_permits); // 分块 worker 各自申请连接许可
        let meta = build_chunk_meta(url, t, ctx.inner.chunk_threshold);
        if let Some(parent) = part_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // 预分配完整大小，分块 worker 各写各的区间
        let f = tokio::fs::File::create(part_path).await?;
        f.set_len(t).await?;
        drop(f);
        let state = PartState::new_arc(meta, meta_path.to_path_buf());
        state.flush().await; // 先落盘断点，中断后可续传
        run_chunked(ctx, url, part_path, state, written).await?;
        finalize_file(req, part_path, meta_path).await.map_err(ChunkErr::Failed)
    } else {
        // 单流下载（本次响应即数据流，无需重新请求）
        let meta = PartMeta {
            url: url.to_string(),
            total,
            chunks: vec![ChunkMeta {
                start: 0,
                end: total.and_then(|t| if t > 0 { Some(t - 1) } else { None }),
                done: 0,
            }],
        };
        let state = PartState::new_arc(meta, meta_path.to_path_buf());
        state.flush().await;
        run_single_fresh(ctx, req, part_path, meta_path, state, resp, written).await
    }
}

/// 单流全新下载：直接消费当前响应体
async fn run_single_fresh(
    ctx: &TaskCtx,
    req: &DownloadRequest,
    part_path: &Path,
    meta_path: &Path,
    state: Arc<PartState>,
    mut resp: reqwest::Response,
    written: &Arc<AtomicU64>,
) -> Result<(), ChunkErr> {
    if let Some(parent) = part_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(part_path)
        .await?;
    // 已知总量则预分配，便于续传时定位写入偏移
    if let Some(t) = state.total() {
        if t > 0 {
            let _ = file.set_len(t).await;
        }
    }
    pump(&mut resp, &mut file, 0, ctx, &state, 0, written).await?;
    let _ = state.flush().await;
    finalize_file(req, part_path, meta_path).await.map_err(ChunkErr::Failed)
}

/// 单流断点续传：Range 续传、200 时从头重下、416 视为已下满
async fn run_single_resume(
    ctx: &TaskCtx,
    req: &DownloadRequest,
    url: &str,
    part_path: &Path,
    meta_path: &Path,
    state: Arc<PartState>,
    written: &Arc<AtomicU64>,
) -> Result<(), ChunkErr> {
    let done = state.chunk(0).done;
    let _permits = ctx.inner.acquire_for(url, ctx.per_host).await?;
    match ctx.ctrl.state() {
        ST_CANCELLED => return Err(ChunkErr::Cancelled),
        ST_PAUSED => return Err(ChunkErr::Paused),
        _ => {}
    }

    let mut resp = ctx
        .inner
        .client
        .get(url)
        .header(header::RANGE, format!("bytes={done}-"))
        .send()
        .await?;
    let status = resp.status();
    let mut file;
    let mut offset = done;
    if status == StatusCode::PARTIAL_CONTENT {
        file = tokio::fs::OpenOptions::new().write(true).open(part_path).await?;
    } else if status == StatusCode::OK {
        // 服务器忽略 Range：从头重下，回退进度
        if done > 0 {
            subtract_progress(&ctx.prog, written, done);
        }
        state.set_done(0, 0);
        offset = 0;
        file = tokio::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(part_path)
            .await?;
    } else if status == StatusCode::RANGE_NOT_SATISFIABLE {
        // Range 不可满足：断点已到文件末尾，直接进入校验
        return finalize_file(req, part_path, meta_path).await.map_err(ChunkErr::Failed);
    } else {
        return Err(ChunkErr::Failed(AppError::msg(format!("服务器返回异常状态 {status}"))));
    }
    file.seek(SeekFrom::Start(offset)).await?;
    pump(&mut resp, &mut file, 0, ctx, &state, 0, written).await?;
    let _ = state.flush().await;
    finalize_file(req, part_path, meta_path).await.map_err(ChunkErr::Failed)
}

/// 分块并发下载：为每个未完成分块派发 worker
async fn run_chunked(
    ctx: &TaskCtx,
    url: &str,
    part_path: &Path,
    state: Arc<PartState>,
    written: &Arc<AtomicU64>,
) -> Result<(), ChunkErr> {
    let n = state.meta.lock().unwrap().chunks.len();
    let mut set: JoinSet<Result<(), ChunkErr>> = JoinSet::new();
    for i in 0..n {
        let ctx = ctx.clone();
        let url = url.to_string();
        let part = part_path.to_path_buf();
        let st = state.clone();
        let wr = written.clone();
        set.spawn(async move { run_chunk(ctx, &url, &part, st, i, &wr).await });
    }

    let mut paused = false;
    let mut cancelled = false;
    let mut failed: Option<ChunkErr> = None;
    while let Some(res) = set.join_next().await {
        let r = match res {
            Ok(r) => r,
            Err(e) if e.is_cancelled() => continue,
            Err(_) => Err(ChunkErr::Failed(AppError::msg("内部下载协程异常退出"))),
        };
        match r {
            Ok(()) => {}
            Err(ChunkErr::Cancelled) => cancelled = true,
            Err(ChunkErr::Paused) => paused = true,
            Err(e) => {
                if failed.is_none() {
                    failed = Some(e);
                    set.abort_all();
                }
            }
        }
    }
    if let Some(e) = failed {
        return Err(e);
    }
    if cancelled {
        return Err(ChunkErr::Cancelled);
    }
    if paused {
        return Err(ChunkErr::Paused);
    }
    Ok(())
}

/// 单分块 worker：Range 请求 + 断点续传
async fn run_chunk(
    ctx: TaskCtx,
    url: &str,
    part_path: &Path,
    state: Arc<PartState>,
    idx: usize,
    written: &AtomicU64,
) -> Result<(), ChunkErr> {
    let chunk = state.chunk(idx);
    if chunk.complete() {
        return Ok(());
    }
    let _permits = ctx.inner.acquire_for(url, ctx.per_host).await?;
    match ctx.ctrl.state() {
        ST_CANCELLED => return Err(ChunkErr::Cancelled),
        ST_PAUSED => return Err(ChunkErr::Paused),
        _ => {}
    }
    let end = chunk
        .end
        .ok_or_else(|| ChunkErr::Failed(AppError::msg("分块缺少结束位置")))?;
    let start = chunk.start + chunk.done;
    if start > end {
        return Ok(());
    }

    let mut resp = ctx
        .inner
        .client
        .get(url)
        .header(header::RANGE, format!("bytes={start}-{end}"))
        .send()
        .await?;
    let status = resp.status();
    if status == StatusCode::OK {
        // 服务器忽略分块 Range 请求：放弃分块模式，回退单流
        return Err(ChunkErr::RangeUnsupported);
    }
    if status != StatusCode::PARTIAL_CONTENT {
        return Err(ChunkErr::Failed(AppError::msg(format!("服务器返回异常状态 {status}"))));
    }

    let mut file = tokio::fs::OpenOptions::new().write(true).open(part_path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    pump(&mut resp, &mut file, start, &ctx, &state, idx, written).await?;
    let _ = state.flush().await;
    Ok(())
}

/// 把响应体流式写入文件（暂停/取消协作式检查；断点按字节阈值落盘）
async fn pump(
    resp: &mut reqwest::Response,
    file: &mut tokio::fs::File,
    base: u64,
    ctx: &TaskCtx,
    state: &PartState,
    idx: usize,
    written: &AtomicU64,
) -> Result<(), ChunkErr> {
    let mut written_total: u64 = 0;
    loop {
        match ctx.ctrl.state() {
            ST_CANCELLED => {
                state.flush().await;
                return Err(ChunkErr::Cancelled);
            }
            ST_PAUSED => {
                state.flush().await;
                return Err(ChunkErr::Paused);
            }
            _ => {}
        }
        let chunk = match timeout(READ_TIMEOUT, resp.chunk()).await {
            Err(_) => {
                state.flush().await;
                return Err(ChunkErr::Failed(AppError::msg("连接超时：30 秒未收到数据")));
            }
            Ok(Err(e)) => {
                state.flush().await;
                return Err(ChunkErr::Failed(e.into()));
            }
            Ok(Ok(None)) => break,
            Ok(Ok(Some(c))) => c,
        };
        if chunk.is_empty() {
            continue;
        }
        file.write_all(&chunk).await?;
        let n = chunk.len() as u64;
        written_total += n;
        ctx.prog.downloaded_bytes.fetch_add(n, Ordering::Relaxed);
        written.fetch_add(n, Ordering::Relaxed);
        state.set_done(idx, base + written_total);
        state.record_bytes(n);
        if state.should_flush() {
            state.flush().await;
        }
    }
    file.flush().await?;
    Ok(())
}

/// 收尾：流式哈希校验 → 原子改名到目标路径
async fn finalize_file(req: &DownloadRequest, part: &Path, meta: &Path) -> AppResult<()> {
    if let Some((algo, expected)) = &req.expected {
        let actual = hash::hash_file(part, *algo).await?;
        if !actual.eq_ignore_ascii_case(expected) {
            wipe_partial(part, meta).await;
            return Err(AppError::msg(format!("哈希校验失败（期望 {expected}，实际 {actual}）")));
        }
    }
    // 校验通过才触碰目标目录；Windows 的 rename 不能覆盖已有文件，先移除损坏旧文件
    let _ = tokio::fs::remove_file(meta).await;
    if let Some(parent) = req.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(&req.dest).await;
    tokio::fs::rename(part, &req.dest).await?;
    Ok(())
}

/// 解析 Content-Range 头中的总长（"bytes 0-1023/2048" → 2048；"*" 视为未知）
fn parse_total(headers: &HeaderMap) -> Option<u64> {
    let v = headers.get(header::CONTENT_RANGE)?.to_str().ok()?;
    v.rsplit('/').next()?.trim().parse::<u64>().ok()
}

/// 按目标块大小（至少阈值，至多 8 块）切分区间
fn build_chunk_meta(url: &str, total: u64, threshold: u64) -> PartMeta {
    let target = threshold.max(total / MAX_CHUNKS).max(1);
    let n = ((total + target - 1) / target).clamp(1, MAX_CHUNKS) as usize;
    let base = total / n as u64;
    let rem = total % n as u64;
    let mut chunks = Vec::with_capacity(n);
    let mut start = 0u64;
    for i in 0..n as u64 {
        let len = base + if i < rem { 1 } else { 0 };
        chunks.push(ChunkMeta { start, end: Some(start + len - 1), done: 0 });
        start += len;
    }
    PartMeta { url: url.to_string(), total: Some(total), chunks }
}

// ---------------------------------------------------------------------------
// 进度事件推送（200ms 节流聚合 + 终态即时推送）
// ---------------------------------------------------------------------------

enum EmitKind {
    Running { force: bool },
    Paused,
    /// completed / failed / cancelled（failed 携带原因）
    Terminal(&'static str, Option<String>),
}

struct EmitState {
    finished: bool,
    last_emit: Option<Instant>,
    last_bytes: u64,
    last_bps: f64,
}

struct EmitCore {
    cb: ProgressCallback,
    task_id: String,
    kind: String,
    instance_id: Option<String>,
    prog: Arc<TaskProgress>,
    state_rx: watch::Receiver<u8>,
    inner: StdMutex<EmitState>,
}

impl EmitCore {
    fn emit(&self, kind: EmitKind) {
        let mut st = self.inner.lock().unwrap();
        match kind {
            EmitKind::Running { force } => {
                if st.finished || *self.state_rx.borrow() != ST_RUNNING {
                    return;
                }
                let now = Instant::now();
                let gap = if force { MIN_EMIT_GAP } else { EMIT_INTERVAL };
                if let Some(t) = st.last_emit {
                    if now.duration_since(t) < gap {
                        return;
                    }
                }
                let bytes = self.prog.downloaded_bytes.load(Ordering::Relaxed);
                let bps = match st.last_emit {
                    Some(t) => {
                        let dt = now.duration_since(t).as_secs_f64();
                        if dt >= 0.1 {
                            bytes.saturating_sub(st.last_bytes) as f64 / dt
                        } else {
                            st.last_bps
                        }
                    }
                    None => 0.0,
                };
                st.last_bps = bps;
                st.last_emit = Some(now);
                st.last_bytes = bytes;
                (self.cb)(self.build("running", bps, None));
            }
            EmitKind::Paused => {
                if st.finished {
                    return;
                }
                // 重置速度采样，避免恢复后速度虚低
                st.last_bytes = self.prog.downloaded_bytes.load(Ordering::Relaxed);
                st.last_bps = 0.0;
                st.last_emit = Some(Instant::now());
                (self.cb)(self.build("paused", 0.0, None));
            }
            EmitKind::Terminal(state, error) => {
                // 终态之后丢弃一切滞留的 running 事件，保证事件顺序正确
                st.finished = true;
                (self.cb)(self.build(state, 0.0, error));
            }
        }
    }

    fn build(&self, state: &str, bps: f64, error: Option<String>) -> DownloadProgress {
        let p = &self.prog;
        let total = p.total_bytes.load(Ordering::Relaxed);
        let mut downloaded = p.downloaded_bytes.load(Ordering::Relaxed);
        // 断点清空重试可能造成短暂超额，展示时夹紧
        if total > 0 {
            downloaded = downloaded.min(total);
        }
        DownloadProgress {
            task_id: self.task_id.clone(),
            kind: self.kind.clone(),
            instance_id: self.instance_id.clone(),
            state: state.to_string(),
            current_file: p.current_file.lock().unwrap().clone(),
            completed_files: p.completed_files.load(Ordering::Relaxed),
            total_files: p.total_files.load(Ordering::Relaxed),
            downloaded_bytes: downloaded,
            total_bytes: total,
            bytes_per_second: bps,
            error,
        }
    }
}

// ---------------------------------------------------------------------------
// 测试：本地极简 HTTP 服务器 + 分块/续传/重试/暂停/取消用例
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::model::HashAlgo;
    use std::sync::mpsc::{channel, Receiver};
    use tokio::io::AsyncReadExt;
    use tokio::net::{TcpListener, TcpStream};

    /// 极简测试服务器：支持 Range（bytes=a-b / a-）、可注入延迟与错误内容
    async fn spawn_test_server(
        data: Arc<Vec<u8>>,
        hits: Arc<StdMutex<Vec<String>>>,
        corrupt: bool,
        delay_ms: u64,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { break };
                let data = data.clone();
                let hits = hits.clone();
                tokio::spawn(handle_conn(sock, data, hits, corrupt, delay_ms));
            }
        });
        addr
    }

    async fn handle_conn(
        mut sock: TcpStream,
        data: Arc<Vec<u8>>,
        hits: Arc<StdMutex<Vec<String>>>,
        corrupt: bool,
        delay_ms: u64,
    ) {
        // 读取请求头
        let mut buf = vec![0u8; 8192];
        let Ok(n) = sock.read(&mut buf).await else { return };
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let range_hdr = req
            .lines()
            .map(str::trim)
            .find(|l| l.to_ascii_lowercase().starts_with("range:"))
            .and_then(|l| l.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_default();
        hits.lock().unwrap().push(range_hdr.clone());

        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }

        let total = data.len() as u64;
        let (start, end) = match range_hdr.strip_prefix("bytes=") {
            Some(spec) => {
                let (a, b) = spec.split_once('-').unwrap_or(("0", ""));
                let s: u64 = a.trim().parse().unwrap_or(0);
                let e: u64 = if b.trim().is_empty() {
                    total - 1
                } else {
                    b.trim().parse::<u64>().unwrap_or(total - 1).min(total - 1)
                };
                (s.min(total - 1), e)
            }
            None => (0, total - 1),
        };
        let len = end - start + 1;
        let payload: Vec<u8> = if corrupt {
            vec![0xAB; len as usize]
        } else {
            data[start as usize..=(end as usize).min(data.len() - 1)].to_vec()
        };

        let head = if range_hdr.is_empty() {
            format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n")
        } else {
            format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\n\
                 Content-Length: {len}\r\nConnection: close\r\n\r\n"
            )
        };
        let _ = sock.write_all(head.as_bytes()).await;
        let _ = sock.write_all(&payload).await;
        let _ = sock.flush().await;
    }

    fn sha256_hex(data: &[u8]) -> String {
        let mut h = Sha256::new();
        Digest::update(&mut h, data);
        hex::encode(Digest::finalize(h))
    }

    /// 等待满足条件的事件；任务失败时直接 panic
    fn wait_until(
        rx: &Receiver<DownloadProgress>,
        want: Option<&str>,
        pred: impl Fn(&DownloadProgress) -> bool,
        secs: u64,
    ) -> DownloadProgress {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let remain = deadline.saturating_duration_since(Instant::now());
            assert!(!remain.is_zero(), "等待事件超时");
            let ev = rx.recv_timeout(remain).expect("接收事件失败");
            if ev.state == "failed" && want != Some("failed") {
                panic!("任务意外失败：{:?}", ev.error);
            }
            if want.is_some_and(|s| s == ev.state) && pred(&ev) {
                return ev;
            }
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piemcl-engine-{tag}-{}", uuid::Uuid::new_v4()));
        // 预创建下载临时目录（生产环境由 ensure_dirs 负责）
        std::fs::create_dir_all(dir.join("tmp")).unwrap();
        dir
    }

    fn make_cb() -> (ProgressCallback, Receiver<DownloadProgress>) {
        let (tx, rx) = channel();
        let cb: ProgressCallback = Arc::new(move |p| {
            let _ = tx.send(p);
        });
        (cb, rx)
    }

    // 多线程运行时：测试线程会阻塞在 std mpsc recv 上，
    // 单线程运行时会导致引擎协程饿死（收不到任何事件）
    #[tokio::test(flavor = "multi_thread")]
    async fn downloads_small_file_and_skips_existing() {
        let root = temp_root("small");
        let engine = DownloadEngine::new_for_test(root.join("tmp"), 8, 4, 1024 * 1024, [0, 0, 0]);
        let data = b"hello pie mcl download engine".to_vec();
        let hits = Arc::new(StdMutex::new(Vec::new()));
        let addr = spawn_test_server(Arc::new(data.clone()), hits.clone(), false, 0).await;

        let dest = root.join("out.bin");
        let mut r = DownloadRequest::new("小文件", vec![format!("http://{addr}/file")], dest.clone());
        r.expected = Some((HashAlgo::Sha256, sha256_hex(&data)));
        r.size = Some(data.len() as u64);

        let (cb, rx) = make_cb();
        engine
            .start_task(
                TaskOptions { task_id: "t1".into(), kind: "test".into(), instance_id: None },
                vec![r],
                cb,
            )
            .await
            .unwrap();

        let ev = wait_until(&rx, Some("completed"), |_| true, 30);
        assert_eq!(ev.completed_files, 1);
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(engine.inner.tmp_dir.read_dir().unwrap().next().is_none(), "完成后临时目录应清空");

        // 再次下载：本地已存在且哈希匹配 → 秒完成，零网络请求
        hits.lock().unwrap().clear();
        let dest2 = dest.clone();
        let r2 = DownloadRequest::new(
            "小文件",
            vec![format!("http://{addr}/file")],
            dest2,
        );
        let (cb2, rx2) = make_cb();
        engine
            .start_task(
                TaskOptions { task_id: "t2".into(), kind: "test".into(), instance_id: None },
                vec![r2],
                cb2,
            )
            .await
            .unwrap();
        let ev2 = wait_until(&rx2, Some("completed"), |_| true, 30);
        assert_eq!(ev2.completed_files, 1);
        assert!(hits.lock().unwrap().is_empty(), "增量跳过不应产生网络请求");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn chunked_download_resumes_from_meta() {
        let root = temp_root("chunk");
        // 4MB 数据、阈值 1MB → 4 块
        let data: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let hits = Arc::new(StdMutex::new(Vec::new()));
        let addr = spawn_test_server(Arc::new(data.clone()), hits.clone(), false, 0).await;
        let url = format!("http://{addr}/file");

        let engine = DownloadEngine::new_for_test(root.join("tmp"), 8, 8, 1024 * 1024, [0, 0, 0]);
        let dest = root.join("big.bin");

        // 手工预置断点：块 0（0–1MB）已完成 0.5MB，模拟上次中断
        // （与真实流程一致：.part 预分配到全尺寸，块数据写在各自偏移）
        let (part, meta) = engine.inner.tmp_paths("t-resume", &dest);
        let mut f = std::fs::File::create(&part).unwrap();
        f.set_len(data.len() as u64).unwrap();
        use std::io::Write as _;
        f.write_all(&data[..524288]).unwrap();
        drop(f);
        let m = PartMeta {
            url: url.clone(),
            total: Some(data.len() as u64),
            chunks: vec![
                ChunkMeta { start: 0, end: Some(1048575), done: 524288 },
                ChunkMeta { start: 1048576, end: Some(2097151), done: 0 },
                ChunkMeta { start: 2097152, end: Some(3145727), done: 0 },
                ChunkMeta { start: 3145728, end: Some(4194303), done: 0 },
            ],
        };
        std::fs::write(&meta, serde_json::to_string(&m).unwrap()).unwrap();

        let r = DownloadRequest::new("大文件", vec![url.clone()], dest.clone());
        let (cb, rx) = make_cb();
        engine
            .start_task(
                TaskOptions { task_id: "t-resume".into(), kind: "test".into(), instance_id: None },
                vec![r],
                cb,
            )
            .await
            .unwrap();

        wait_until(&rx, Some("completed"), |_| true, 60);
        // 续传后文件完整
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        // 块 0 必须从 0.5MB 处续传（而非从 0 开始）
        let all = hits.lock().unwrap().join(" | ");
        assert!(
            all.contains("bytes=524288-1048575"),
            "块 0 应从断点 524288 续传，实际请求：{all}"
        );
        // 4 个分块各 1 次请求（无探测请求）
        assert_eq!(hits.lock().unwrap().len(), 4, "应恰好 4 次分块请求：{all}");
        // 完成后临时目录清空
        assert!(engine.inner.tmp_dir.read_dir().unwrap().next().is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hash_mismatch_retries_then_fails() {
        let root = temp_root("hash");
        let data = b"correct content for hash check".to_vec();
        let hits = Arc::new(StdMutex::new(Vec::new()));
        // corrupt=true：服务器返回错误内容 → 哈希必不匹配
        let addr = spawn_test_server(Arc::new(data.clone()), hits.clone(), true, 0).await;

        let engine = DownloadEngine::new_for_test(root.join("tmp"), 4, 4, 1024 * 1024, [0, 0, 0]);
        let dest = root.join("bad.bin");
        let mut r = DownloadRequest::new("坏文件", vec![format!("http://{addr}/file")], dest.clone());
        r.expected = Some((HashAlgo::Sha256, sha256_hex(&data)));

        let (cb, rx) = make_cb();
        engine
            .start_task(
                TaskOptions { task_id: "t-hash".into(), kind: "test".into(), instance_id: None },
                vec![r],
                cb,
            )
            .await
            .unwrap();

        let ev = wait_until(&rx, Some("failed"), |_| true, 30);
        assert!(ev.error.unwrap_or_default().contains("哈希校验失败"));
        // 1 次初始 + 3 次重试 = 4 次尝试
        assert_eq!(hits.lock().unwrap().len(), 4, "哈希不匹配应重试 3 次（共 4 次尝试）");
        // 失败后目标文件不应存在，断点已清理
        assert!(!dest.exists());
        assert!(engine.inner.tmp_dir.read_dir().unwrap().next().is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pause_resume_then_cancel_works() {
        let root = temp_root("pause");
        // 每文件 300KB × 40 个，服务器每请求延迟 80ms → 全程约 0.8s+，留出暂停窗口
        let data: Vec<u8> = (0..300 * 1024).map(|i| (i % 199) as u8).collect();
        let hits = Arc::new(StdMutex::new(Vec::new()));
        let addr = spawn_test_server(Arc::new(data.clone()), hits.clone(), false, 80).await;
        let url = format!("http://{addr}/file");

        let engine = DownloadEngine::new_for_test(root.join("tmp"), 4, 4, 1024 * 1024, [0, 0, 0]);
        let requests: Vec<DownloadRequest> = (0..40)
            .map(|i| DownloadRequest::new(format!("文件{i}"), vec![url.clone()], root.join(format!("f{i}.bin"))))
            .collect();

        let (cb, rx) = make_cb();
        engine
            .start_task(
                TaskOptions { task_id: "t-pause".into(), kind: "test".into(), instance_id: None },
                requests,
                cb,
            )
            .await
            .unwrap();

        // 等首批文件完成，然后暂停
        wait_until(&rx, Some("running"), |ev| ev.completed_files >= 4, 30);
        engine.pause_task("t-pause").await.unwrap();
        let ev = wait_until(&rx, Some("paused"), |_| true, 30);
        assert_eq!(ev.state, "paused");

        // 恢复 → 全部完成
        engine.resume_task("t-pause").await.unwrap();
        let done = wait_until(&rx, Some("completed"), |_| true, 60);
        assert_eq!(done.completed_files, 40, "恢复后应完成全部 40 个文件");
        assert_eq!(std::fs::read(root.join("f39.bin")).unwrap(), data);

        // 再来一个任务验证取消：进行中取消 → 断点清理、无残留
        let requests2: Vec<DownloadRequest> = (0..40)
            .map(|i| DownloadRequest::new(format!("文件{i}"), vec![url.clone()], root.join(format!("g{i}.bin"))))
            .collect();
        let (cb2, rx2) = make_cb();
        engine
            .start_task(
                TaskOptions { task_id: "t-cancel".into(), kind: "test".into(), instance_id: None },
                requests2,
                cb2,
            )
            .await
            .unwrap();
        wait_until(&rx2, Some("running"), |ev| ev.completed_files >= 4, 30);
        engine.cancel_task("t-cancel").await.unwrap();
        let ev = wait_until(&rx2, Some("cancelled"), |_| true, 30);
        assert_eq!(ev.state, "cancelled");
        // 取消瞬间允许在途文件自然收尾（内容完整有效），
        // 但绝不允许全部 40 个完成 —— 确定性不变式
        let existing = (0..40).filter(|i| root.join(format!("g{i}.bin")).exists()).count();
        assert!(existing < 40, "取消后不应完成全部文件（已完成 {existing}/40）");
        assert!(engine.inner.tmp_dir.read_dir().unwrap().next().is_none(), "取消后临时目录应清空");

        let _ = std::fs::remove_dir_all(&root);
    }
}
