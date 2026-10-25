// 游戏进程管理：spawn、逐行日志捕获、强制结束、内存查询
//
// - spawn_game：启动 Java 进程（CREATE_NO_WINDOW 防止控制台黑窗），
//   stdout/stderr 逐行回调 + 最近 200 行环形缓冲（退出时落盘 latest.log），
//   独立 wait 任务监视退出码
// - terminate：Windows TerminateProcess 强制结束（wait 任务随之自然收尾）
// - process_memory：sysinfo 查询进程 RSS（前端运行状态面板轮询）

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use super::args::LaunchPlan;
use crate::core::error::{AppError, AppResult};

/// 日志环形缓冲容量（latest.log 记录的尾部行数）
const LOG_TAIL_CAPACITY: usize = 200;
/// CREATE_NO_WINDOW：隐藏 Java 控制台黑窗
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 运行中的游戏进程
#[derive(Debug)]
pub struct RunningProcess {
    pub pid: u32,
    /// 启动时间（epoch 秒）
    pub started_at: i64,
    /// 最近 200 行日志（stdout/stderr 合并，行首带 [stdout]/[stderr] 标记）
    pub log_tail: Arc<StdMutex<VecDeque<String>>>,
}

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 启动游戏进程并接管其输出。
///
/// - `on_log(stream, line)`：每收到一行回调一次（前端实时日志）
/// - `on_exit(code, log_tail)`：进程退出时回调一次（code=None 表示被强杀等
///   无法取码的场景），随后内部 wait 任务结束
pub fn spawn_game(
    plan: &LaunchPlan,
    on_log: Arc<dyn Fn(&str, String) + Send + Sync>,
    on_exit: impl FnOnce(Option<i32>, Vec<String>) + Send + 'static,
) -> AppResult<Arc<RunningProcess>> {
    let mut cmd = Command::new(&plan.program);
    cmd.args(&plan.args)
        .current_dir(&plan.working_dir)
        .envs(&plan.envs)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::msg(format!("启动 Java 进程失败：{e}（请检查 Java 路径与权限）")))?;
    let pid = child
        .id()
        .ok_or_else(|| AppError::msg("启动 Java 进程失败：无法获取进程 ID"))?;

    let log_tail: Arc<StdMutex<VecDeque<String>>> =
        Arc::new(StdMutex::new(VecDeque::with_capacity(LOG_TAIL_CAPACITY)));

    let push_line = |log_tail: &StdMutex<VecDeque<String>>, stream: &str, line: String| {
        let mut tail = log_tail.lock().unwrap();
        if tail.len() >= LOG_TAIL_CAPACITY {
            tail.pop_front();
        }
        tail.push_back(format!("[{stream}] {line}"));
    };

    // stdout 逐行捕获（read_until + lossy 解码：Java 错误输出可能是 GBK 等
    // 非 UTF-8 编码，lines() 遇无效字节会 Err 中断导致日志整段丢失）
    let mut stdout_task: Option<tokio::task::JoinHandle<()>> = None;
    if let Some(out) = child.stdout.take() {
        let tail = log_tail.clone();
        let on_log = on_log.clone();
        stdout_task = Some(tokio::spawn(async move {
            let mut reader = BufReader::new(out);
            let mut buf: Vec<u8> = Vec::with_capacity(256);
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf)
                            .trim_end_matches(['\r', '\n'])
                            .to_string();
                        push_line(&tail, "stdout", line.clone());
                        on_log("stdout", line);
                    }
                }
            }
        }));
    }
    // stderr 逐行捕获（Minecraft 主要日志走 stderr）
    let mut stderr_task: Option<tokio::task::JoinHandle<()>> = None;
    if let Some(err) = child.stderr.take() {
        let tail = log_tail.clone();
        let on_log = on_log.clone();
        stderr_task = Some(tokio::spawn(async move {
            let mut reader = BufReader::new(err);
            let mut buf: Vec<u8> = Vec::with_capacity(256);
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf)
                            .trim_end_matches(['\r', '\n'])
                            .to_string();
                        push_line(&tail, "stderr", line.clone());
                        on_log("stderr", line);
                    }
                }
            }
        }));
    }

    // 退出监视任务（独占 Child）：等管道读取任务收尾后再取尾部日志，
    // 避免进程退出与缓冲区消费的竞态导致日志缺失
    let running = Arc::new(RunningProcess {
        pid,
        started_at: now_epoch(),
        log_tail: log_tail.clone(),
    });
    let running_for_exit = running.clone();
    tokio::spawn(async move {
        let code = child.wait().await.ok().and_then(|s| s.code());
        if let Some(h) = stdout_task {
            let _ = h.await;
        }
        if let Some(h) = stderr_task {
            let _ = h.await;
        }
        let tail: Vec<String> =
            running_for_exit.log_tail.lock().unwrap().iter().cloned().collect();
        on_exit(code, tail);
    });

    Ok(running)
}

/// 强制结束进程（Windows TerminateProcess）
pub fn terminate(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if handle.is_null() {
                return Err(format!("打开进程 {pid} 失败（可能已自行退出）"));
            }
            let ok = TerminateProcess(handle, 1);
            CloseHandle(handle);
            if ok == 0 {
                return Err(format!("结束进程 {pid} 失败"));
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Err("当前平台不支持强制结束进程".into())
    }
}

/// 查询进程内存占用（RSS 字节）；进程已退出返回 None
pub fn process_memory(pid: u32) -> Option<u64> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory())
}
