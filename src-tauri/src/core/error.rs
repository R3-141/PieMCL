// Pie MCL 统一错误类型
//
// 约定：所有业务模块返回 AppResult<T>；Tauri command 层将其转为
// Result<T, String>（中文可读文案）后抛给前端。

/// 业务错误
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),

    #[error("文件读写失败：{0}")]
    Io(#[from] std::io::Error),

    #[error("数据解析失败：{0}")]
    Json(#[from] serde_json::Error),

    #[error("回收站操作失败：{0}")]
    Trash(#[from] trash::Error),

    /// 业务校验 / 逻辑错误，构造时直接给中文文案
    #[error("{0}")]
    Msg(String),
}

impl AppError {
    /// 快速构造业务错误
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Msg(s.into())
    }
}

/// 供 Tauri command 使用：错误转中文文案
impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}

pub type AppResult<T> = Result<T, AppError>;
