// 下载引擎数据模型
//
// DownloadRequest：单个目标文件的下载请求（多 URL 候选 = 镜像优先、官方兜底）；
// DownloadProgress：推送给前端的结构化进度事件 payload（禁止前端轮询）。

use std::path::PathBuf;

use serde::Serialize;

/// 哈希算法
/// （SHA-256/512 供后续 Modrinth / CurseForge 资源安装使用）
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgo {
    Sha1,
    Sha256,
    Sha512,
}

impl HashAlgo {
    /// 序列化为事件/日志用小写名称（后续资源安装阶段的进度展示使用）
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            HashAlgo::Sha1 => "sha1",
            HashAlgo::Sha256 => "sha256",
            HashAlgo::Sha512 => "sha512",
        }
    }
}

/// 单文件下载请求
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    /// 任务内唯一键（同时用作进度里的当前文件显示名）
    pub key: String,
    /// URL 候选列表（按序尝试：镜像优先、官方兜底）
    pub urls: Vec<String>,
    /// 目标绝对路径（原子写入：先写 .part 再 rename）
    pub dest: PathBuf,
    /// 期望哈希（算法, 小写 hex）；None 表示不校验
    pub expected: Option<(HashAlgo, String)>,
    /// 期望大小（字节），用于总进度估算；None 表示未知
    pub size: Option<u64>,
}

impl DownloadRequest {
    /// 快速构造（常用于测试与简单场景）
    pub fn new(key: impl Into<String>, urls: Vec<String>, dest: PathBuf) -> Self {
        Self {
            key: key.into(),
            urls,
            dest,
            expected: None,
            size: None,
        }
    }
}

/// 下载进度事件 payload（download-progress 事件）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub task_id: String,
    /// 任务类型：resources / loader / java
    pub kind: String,
    pub instance_id: Option<String>,
    /// running / paused / completed / failed / cancelled
    pub state: String,
    /// 正在下载的文件显示名
    pub current_file: Option<String>,
    /// 已完成文件数（含本地校验跳过）
    pub completed_files: u64,
    pub total_files: u64,
    pub downloaded_bytes: u64,
    /// 预估总字节数（各文件期望大小之和；未知部分计 0）
    pub total_bytes: u64,
    /// 实时速度（字节/秒，200ms 窗口均值）
    pub bytes_per_second: f64,
    /// 失败原因（仅 failed 状态）
    pub error: Option<String>,
}
