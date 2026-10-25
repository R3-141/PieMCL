// 服务器收藏列表：{启动器目录}/servers.json
//
// 启动器级数据（与实例无关）：id / 备注名 / 地址。
// 文件缺失或损坏时静默回落空列表（收藏损坏不应阻塞页面）。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::error::AppResult;

/// 单个收藏服务器
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    pub id: String,
    /// 备注名（缺省展示地址）
    pub name: String,
    /// 地址（host 或 host:port）
    pub address: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ServerFile {
    #[serde(default)]
    servers: Vec<ServerEntry>,
}

pub fn load(root: &Path) -> Vec<ServerEntry> {
    let Ok(text) = std::fs::read_to_string(root.join("servers.json")) else {
        return Vec::new();
    };
    serde_json::from_str::<ServerFile>(&text)
        .unwrap_or_default()
        .servers
}

pub fn save(root: &Path, servers: &[ServerEntry]) -> AppResult<()> {
    let text = serde_json::to_string_pretty(&ServerFile {
        servers: servers.to_vec(),
    })
    .map_err(|e| crate::core::error::AppError::msg(format!("序列化服务器列表失败：{e}")))?;
    std::fs::write(root.join("servers.json"), text)
        .map_err(|e| crate::core::error::AppError::Io(e))?;
    Ok(())
}

/// 地址格式校验：host[:port]，端口 1–65535；host 不含空白与协议前缀
pub fn validate_address(address: &str) -> Result<(), String> {
    let a = address.trim();
    if a.is_empty() {
        return Err("服务器地址不能为空".into());
    }
    if a.contains(char::is_whitespace) {
        return Err("服务器地址不能包含空白字符".into());
    }
    if a.starts_with("http://") || a.starts_with("https://") {
        return Err("地址不需要协议前缀，请直接填 host 或 host:端口".into());
    }
    if let Some((_h, p)) = a.rsplit_once(':') {
        if !p.is_empty() {
            let port: u16 = p
                .parse()
                .map_err(|_| format!("端口无效：{p}（应为 1–65535）"))?;
            if port == 0 {
                return Err("端口无效：0".into());
            }
        }
    }
    Ok(())
}
