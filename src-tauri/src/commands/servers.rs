// 服务器页 commands：收藏列表 CRUD + ping
//
// 列表持久化于 {启动器目录}/servers.json；ping 走 Server List Ping 协议
// （servers/ping.rs，含 SRV 解析与 MOTD 统一转 § 码）。

use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::servers::{ping, store::ServerEntry};

/// 校验地址格式（添加 / 编辑共用）
fn validate(name: &str, address: &str) -> Result<(String, String), String> {
    let name = name.trim();
    let address = address.trim();
    if name.is_empty() {
        return Err("服务器名称不能为空".into());
    }
    if name.chars().count() > 32 {
        return Err("服务器名称过长（最多 32 字符）".into());
    }
    crate::servers::store::validate_address(address)?;
    Ok((name.to_string(), address.to_string()))
}

#[tauri::command]
pub async fn list_servers(state: State<'_, AppState>) -> Result<Vec<ServerEntry>, String> {
    let root = state.paths.root().to_path_buf();
    tokio::task::spawn_blocking(move || crate::servers::store::load(&root))
        .await
        .map_err(|e| format!("读取任务执行失败：{e}"))
}

#[tauri::command]
pub async fn add_server(
    state: State<'_, AppState>,
    name: Option<String>,
    address: String,
) -> Result<ServerEntry, String> {
    let address = address.trim();
    let name = match name {
        Some(n) => n,
        None => address.to_string(), // 缺省用地址作名称
    };
    let (name, address) = validate(&name, address)?;
    let mut servers = {
        let root = state.paths.root().to_path_buf();
        tokio::task::spawn_blocking(move || crate::servers::store::load(&root))
            .await
            .map_err(|e| format!("读取任务执行失败：{e}"))?
    };
    // 同地址去重
    if servers.iter().any(|s| s.address.eq_ignore_ascii_case(&address)) {
        return Err("该服务器已在列表中".into());
    }
    let entry = ServerEntry {
        id: uuid::Uuid::new_v4().simple().to_string(),
        name,
        address: address.to_string(),
    };
    servers.push(entry.clone());
    let root = state.paths.root().to_path_buf();
    tokio::task::spawn_blocking(move || crate::servers::store::save(&root, &servers))
        .await
        .map_err(|e| format!("保存任务执行失败：{e}"))?
        .map_err(String::from)?;
    Ok(entry)
}

#[tauri::command]
pub async fn update_server(
    state: State<'_, AppState>,
    id: String,
    name: String,
    address: String,
) -> Result<(), String> {
    let (name, address) = validate(&name, &address)?;
    let mut servers = {
        let root = state.paths.root().to_path_buf();
        tokio::task::spawn_blocking(move || crate::servers::store::load(&root))
            .await
            .map_err(|e| format!("读取任务执行失败：{e}"))?
    };
    let Some(entry) = servers.iter_mut().find(|s| s.id == id) else {
        return Err(format!("服务器 {id} 不存在"));
    };
    entry.name = name;
    entry.address = address;
    let root = state.paths.root().to_path_buf();
    tokio::task::spawn_blocking(move || crate::servers::store::save(&root, &servers))
        .await
        .map_err(|e| format!("保存任务执行失败：{e}"))?
        .map_err(String::from)
}

#[tauri::command]
pub async fn remove_server(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut servers = {
        let root = state.paths.root().to_path_buf();
        tokio::task::spawn_blocking(move || crate::servers::store::load(&root))
            .await
            .map_err(|e| format!("读取任务执行失败：{e}"))?
    };
    let before = servers.len();
    servers.retain(|s| s.id != id);
    if servers.len() == before {
        return Err(format!("服务器 {id} 不存在"));
    }
    let root = state.paths.root().to_path_buf();
    tokio::task::spawn_blocking(move || crate::servers::store::save(&root, &servers))
        .await
        .map_err(|e| format!("保存任务执行失败：{e}"))?
        .map_err(String::from)
}

/// ping 结果（前端展示；成功才返回，失败为 Err 中文原因）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPingView {
    pub latency_ms: u32,
    pub version_name: String,
    pub protocol: i64,
    pub players_online: u64,
    pub players_max: u64,
    pub favicon: Option<String>,
    pub motd: String,
    pub forge: bool,
    /// 在线玩家名单（players.sample；空 = 服务器未提供）
    pub player_sample: Vec<String>,
}

#[tauri::command]
pub async fn ping_server(address: String) -> Result<ServerPingView, String> {
    let address = address.trim().to_string();
    crate::servers::store::validate_address(&address)?;
    let r: ping::ServerPingResult = ping::ping(&address).await?;
    Ok(ServerPingView {
        latency_ms: r.latency_ms,
        version_name: r.version_name,
        protocol: r.protocol,
        players_online: r.players_online,
        players_max: r.players_max,
        favicon: r.favicon,
        motd: r.motd,
        forge: r.forge,
        player_sample: r.player_sample,
    })
}

/// 导入结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// 新导入条数
    pub imported: u32,
    /// 因实例已有同地址而跳过的条数
    pub skipped: u32,
}

/// 把启动器收藏的全部服务器导入指定实例的服务器列表（servers.dat，
/// 按地址去重合并；实例运行中时游戏可能覆盖写入，请先关闭对应实例）
#[tauri::command]
pub async fn export_servers_to_instance(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<ImportResult, String> {
    let root = state.paths.root().to_path_buf();
    let servers = tokio::task::spawn_blocking(move || crate::servers::store::load(&root))
        .await
        .map_err(|e| format!("读取任务执行失败：{e}"))?;
    if servers.is_empty() {
        return Err("收藏列表为空，没有可导入的服务器".into());
    }
    let game_dir = state
        .instances
        .game_dir(&instance_id)
        .await
        .map_err(String::from)?;
    let (imported, skipped) =
        tokio::task::spawn_blocking(move || crate::servers::nbt::merge_to_instance_dat(&game_dir, &servers))
            .await
            .map_err(|e| format!("导入任务执行失败：{e}"))?
            ?;
    Ok(ImportResult {
        imported: imported as u32,
        skipped: skipped as u32,
    })
}
