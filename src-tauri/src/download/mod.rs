// 下载引擎模块
//
// - model:  下载请求与进度事件数据结构
// - hash:   流式哈希计算与校验（禁止整读内存）
// - mirror: Mojang 官方域名的镜像源改写（BMCLAPI 路径方案）
// - engine: 并发调度、分块下载、断点续传、暂停/继续/取消

pub mod engine;
pub mod hash;
pub mod mirror;
pub mod model;

pub use engine::{DownloadEngine, ProgressCallback, TaskOptions};
