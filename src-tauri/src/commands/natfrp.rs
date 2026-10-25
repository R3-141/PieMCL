// 联机（樱花内网穿透）commands：状态 / 配置 / frpc 下载 / 隧道启停
//
// frpc 进程：`frpc -f <访问密钥>:<隧道ID>`（樱花定制版用法），
// 全局同时仅允许一个 frpc 实例（AppState.natfrp 槽）。
// 日志经 natfrp-log 事件逐行推送；退出经 natfrp-state 事件通知前端。

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::AppState;
use crate::natfrp::NatFrpConfig;

/// frpc 退出码读取等待（强杀后给一点收尾时间）
const STOP_GRACE: Duration = Duration::from_secs(3);

/// 联机页状态视图
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NatFrpStatus {
    /// frpc 可执行文件是否就绪
    pub frpc_ready: bool,
    /// 实际使用的 frpc 路径（展示用）
    pub frpc_path: String,
    /// frpc 路径是否为用户手动指定的
    pub frpc_custom: bool,
    /// 是否已配置访问密钥
    pub access_key_set: bool,
    /// 访问密钥明文（回显输入框，配置实际存于 config.json）
    pub access_key: String,
    /// 已保存的隧道 ID（回显输入框）
    pub tunnel_id: String,
    /// frpc 是否运行中
    pub running: bool,
}

#[tauri::command]
pub async fn natfrp_status(state: State<'_, AppState>) -> Result<NatFrpStatus, String> {
    let config = NatFrpConfig::load(&state.paths);
    let running = state.natfrp.lock().unwrap().is_some();
    let path = config.frpc_path(&state.paths);
    Ok(NatFrpStatus {
        frpc_ready: config.frpc_ready(&state.paths),
        frpc_path: path.to_string_lossy().into_owned(),
        frpc_custom: config.frpc_path.is_some(),
        access_key_set: !config.access_key.trim().is_empty(),
        access_key: config.access_key.clone(),
        tunnel_id: config.tunnel_id.clone(),
        running,
    })
}

/// 保存联机配置（访问密钥 + 隧道 ID + 可选的 frpc 路径覆盖）
#[tauri::command]
pub async fn natfrp_save_config(
    state: State<'_, AppState>,
    access_key: String,
    tunnel_id: String,
    frpc_path: Option<String>,
) -> Result<(), String> {
    let mut config = NatFrpConfig::load(&state.paths);
    config.access_key = access_key.trim().to_string();
    config.tunnel_id = tunnel_id.trim().to_string();
    config.frpc_path = frpc_path
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    config.save(&state.paths)
}

/// 用系统默认浏览器打开樱花面板页面（仅允许 https 链接）
#[tauri::command]
pub async fn natfrp_open_page(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("仅允许打开 https 链接".into());
    }
    opener::open(&url).map_err(|e| format!("打开浏览器失败：{e}"))
}

/// 手动选择 frpc.exe（文件对话框），校验存在后写入配置
#[tauri::command]
pub async fn natfrp_pick_frpc(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("frpc 可执行文件", &["exe"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("对话框任务执行失败：{e}"))?;
    let Some(picked) = picked else {
        return Err("已取消选择".into());
    };
    let path = picked
        .into_path()
        .map_err(|e| format!("获取路径失败：{e}"))?;
    if !path.is_file() {
        return Err("所选路径不是文件".into());
    }
    let mut config = NatFrpConfig::load(&state.paths);
    config.frpc_path = Some(path.to_string_lossy().into_owned());
    config.save(&state.paths)?;
    Ok(path.to_string_lossy().into_owned())
}

/// 自动下载 frpc：樱花 API 解析直链 → 下载引擎（进度走下载面板，互斥键 natfrp:frpc）
/// → 按 MZ/zip 魔数提取 frpc.exe 落位默认路径（清除手动指定，统一用下载版）
#[tauri::command]
pub async fn natfrp_download_frpc(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let dl = crate::natfrp::resolve_frpc_download().await;
    let tmp = state.paths.tmp_dir().join("natfrp-frpc.bin");
    // 清理旧残留：下载引擎对无哈希任务「文件存在即跳过」，残留会把旧错误文件当下载完成
    let _ = tokio::fs::remove_file(&tmp).await;
    let mut req = crate::download::model::DownloadRequest::new(
        "樱花 frpc 客户端".to_string(),
        dl.urls,
        tmp.clone(),
    );
    req.size = dl.size;

    let lock_key = "natfrp:frpc";
    let task_id = format!("natfrp-frpc-{}", uuid::Uuid::new_v4().simple());
    super::register_retry(
        &state.retry_defs,
        &task_id,
        crate::download::TaskOptions {
            task_id: task_id.clone(),
            kind: "natfrp".into(),
            instance_id: Some(lock_key.to_string()),
        },
        vec![req.clone()],
        Some(lock_key.to_string()),
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let tx = StdMutex::new(Some(tx));
    let cb = super::make_retry_cb(
        state.retry_defs.clone(),
        state.active_downloads.clone(),
        app.clone(),
        task_id.clone(),
        Some(tx),
    );
    state
        .download
        .start_task(
            crate::download::TaskOptions {
                task_id: task_id.clone(),
                kind: "natfrp".into(),
                instance_id: Some(lock_key.to_string()),
            },
            vec![req],
            cb,
        )
        .await?;
    // 等待下载终态
    rx.await
        .map_err(|_| "frpc 下载任务意外结束".to_string())?
        .map_err(String::from)?;

    // 提取落位（阻塞操作放 spawn_blocking）；失败时清理残留并附文件头 hex 便于诊断
    let downloaded = tmp.clone();
    let dest_dir = state.paths.natfrp_dir();
    if let Err(e) = tauri::async_runtime::spawn_blocking(move || {
        crate::natfrp::extract_frpc(&downloaded, &dest_dir)
    })
    .await
    .map_err(|e| format!("提取任务执行失败：{e}"))?
    {
        let head = tokio::fs::read(&tmp)
            .await
            .ok()
            .map(|data| {
                data.iter()
                    .take(16)
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| "无法读取".into());
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("{e}（文件头：{head}）"));
    }

    // 下载版落位默认路径：清除手动指定（如无必要不覆盖用户选择——仅当手动路径也是默认路径时无影响）
    let mut config = NatFrpConfig::load(&state.paths);
    if config.frpc_path.is_some() && config.frpc_path.as_deref() == Some(state.paths.natfrp_frpc_path().to_string_lossy().as_ref()) {
        config.frpc_path = None;
        config.save(&state.paths)?;
    }
    let _ = tokio::fs::remove_file(&tmp).await;
    Ok(())
}

/// 启动隧道：frpc -f <密钥>:<隧道ID>；日志经 natfrp-log 推送
#[tauri::command]
pub async fn natfrp_start(
    app: AppHandle,
    state: State<'_, AppState>,
    tunnel_id: String,
) -> Result<(), String> {
    let tunnel_id = tunnel_id.trim().to_string();
    if tunnel_id.is_empty() || !tunnel_id.chars().all(|c| c.is_ascii_digit()) {
        return Err("隧道 ID 无效（应为纯数字，见樱花面板隧道列表）".into());
    }
    let config = NatFrpConfig::load(&state.paths);
    if config.access_key.trim().is_empty() {
        return Err("尚未配置访问密钥（natfrp.com 面板「访问密钥」页获取）".into());
    }
    let frpc_path = config.frpc_path(&state.paths);
    if !frpc_path.is_file() {
        return Err("frpc 未就绪，请先下载或手动指定 frpc.exe".into());
    }
    {
        let mut slot = state.natfrp.lock().unwrap();
        if slot.is_some() {
            return Err("穿透已在运行中".into());
        }
    }

    let mut cmd = tokio::process::Command::new(&frpc_path);
    cmd.arg("-f")
        .arg(format!("{}:{}", config.access_key.trim(), tunnel_id))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    // GUI 进程派生控制台程序必须隐藏黑窗（tokio Command 自带 creation_flags）
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 frpc 失败：{e}"))?;

    // 日志转发：stdout / stderr 各一个读取任务（类型不同无法合并），GBK 容错。
    // 逐行提取樱花隧道连接地址（*.natfrp.*:端口），新地址做 A/AAAA 解析后
    // 经 natfrp-addr 事件推送（前端展示 IPv4 / IPv6 直连地址条）。
    let app_log = app.clone();
    let seen_addrs: Arc<StdMutex<std::collections::HashSet<String>>> =
        Arc::new(StdMutex::new(std::collections::HashSet::new()));
    let stdout = child
        .stdout
        .take()
        .map(|r| Box::new(r) as Box<dyn tokio::io::AsyncRead + Send + Unpin>);
    let stderr = child
        .stderr
        .take()
        .map(|r| Box::new(r) as Box<dyn tokio::io::AsyncRead + Send + Unpin>);
    for reader in [stdout, stderr].into_iter().flatten() {
        let app2 = app_log.clone();
        let seen = seen_addrs.clone();
        let mut reader = BufReader::new(reader);
        tauri::async_runtime::spawn(async move {
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                        let _ = app2.emit("natfrp-log", serde_json::json!({ "line": line }));
                        // 提取连接地址（新出现的才解析与推送）
                        for addr in crate::natfrp::extract_addresses(&line) {
                            if !seen.lock().unwrap().insert(addr.clone()) {
                                continue;
                            }
                            let app3 = app2.clone();
                            tauri::async_runtime::spawn(async move {
                                let (host, port) = match addr.rsplit_once(':') {
                                    Some((h, p)) => (h.to_string(), p.to_string()),
                                    None => return,
                                };
                                let (v4, _v6) = crate::natfrp::resolve_host_ips(&host).await;
                                let _ = app3.emit(
                                    "natfrp-addr",
                                    serde_json::json!({
                                        "address": addr,
                                        "ipv4": v4.iter().map(|ip| format!("{ip}:{port}")).collect::<Vec<_>>(),
                                    }),
                                );
                            });
                        }
                    }
                }
            }
        });
    }

    // 注册运行槽 + 退出监视
    let exited_app = app.clone();
    let slot = state.natfrp.clone();
    slot.lock().unwrap().replace(child);

    tauri::async_runtime::spawn(async move {
        // 等待退出：从槽中 take 出 child（stop 时槽已清空并自行 kill）
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let taken = {
                let mut s = slot.lock().unwrap();
                if let Some(child) = s.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            let c = s.take();
                            Some((Some(status), c))
                        }
                        Ok(None) => continue,
                        Err(_) => {
                            let c = s.take();
                            Some((None, c))
                        }
                    }
                } else {
                    None // 已被 stop 移除：退出监视结束（stop 流程自行上报）
                }
            };
            if let Some((status, _child)) = taken {
                let code = status.and_then(|s| s.code());
                let _ = exited_app.emit(
                    "natfrp-state",
                    serde_json::json!({ "running": false, "code": code }),
                );
                break;
            }
        }
    });

    let _ = app.emit("natfrp-state", serde_json::json!({ "running": true }));
    Ok(())
}

/// 停止穿透：强杀 frpc 进程
#[tauri::command]
pub async fn natfrp_stop(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut child = state
        .natfrp
        .lock()
        .unwrap()
        .take()
        .ok_or("穿透未在运行")?;
    let _ = child.kill().await;
    // 退出监视任务发现槽已空会自行结束；这里直接上报停止态
    let _ = app.emit("natfrp-state", serde_json::json!({ "running": false }));
    Ok(())
}

/// 当前 frpc 进程 PID（诊断用；未运行返回 None）
#[tauri::command]
pub async fn natfrp_pid(state: State<'_, AppState>) -> Result<Option<u32>, String> {
    Ok(state.natfrp.lock().unwrap().as_ref().and_then(|c| c.id()))
}

/// 隧道连接地址（IPv4 已解析；仅保留 IPv4，用户需求：留一个 IPv4 即可）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NatFrpAddr {
    pub address: String,
    pub ipv4: Vec<String>,
}

/// 按访问密钥从樱花隧道列表 API 拉取连接地址。
/// frpc 标准日志不打印远程地址，改由隧道 API 获取（前端启动后轮询合并）。
#[tauri::command]
pub async fn natfrp_fetch_tunnel(state: State<'_, AppState>) -> Result<Vec<NatFrpAddr>, String> {
    let config = NatFrpConfig::load(&state.paths);
    let key = config.access_key.trim().to_string();
    if key.is_empty() {
        return Err("尚未配置访问密钥".into());
    }
    let addrs = crate::natfrp::fetch_tunnel_addrs(&key).await?;
    let mut out = Vec::new();
    for addr in addrs {
        let (host, port) = match addr.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.to_string()),
            None => continue,
        };
        let (v4, _v6) = crate::natfrp::resolve_host_ips(&host).await;
        out.push(NatFrpAddr {
            address: addr,
            ipv4: v4.iter().map(|ip| format!("{ip}:{port}")).collect(),
        });
    }
    Ok(out)
}

/// 隧道信息（列表展示用）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NatFrpTunnel {
    pub id: u64,
    pub name: String,
    pub tunnel_type: String,
    pub node_id: u64,
    pub node_name: String,
    pub online: bool,
    pub banned: bool,
    pub status_reason: String,
    pub local_ip: String,
    pub local_port: u32,
    pub remote: String,
}

/// 节点信息（新建隧道选择用）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NatFrpNode {
    pub id: u64,
    pub name: String,
    pub host: String,
    pub description: String,
    /// 允许创建隧道（满载时为 false）
    pub can_create: bool,
    pub allow_udp: bool,
    /// 允许穿透公开游戏服务器（flag&2048 或名称含"多线"）
    pub allow_public_game: bool,
    pub offline: bool,
    pub is_private: bool,
}

fn require_access_key(state: &State<'_, AppState>) -> Result<String, String> {
    let config = NatFrpConfig::load(&state.paths);
    let key = config.access_key.trim().to_string();
    if key.is_empty() {
        return Err("尚未配置访问密钥".into());
    }
    Ok(key)
}

/// 隧道列表（合并节点名；隐藏 wol 类型）
#[tauri::command]
pub async fn natfrp_list_tunnels(
    state: State<'_, AppState>,
) -> Result<Vec<NatFrpTunnel>, String> {
    let key = require_access_key(&state)?;
    let (tunnels_v, nodes_v) = tokio::join!(
        crate::natfrp::api_get(&key, "/tunnels"),
        crate::natfrp::api_get(&key, "/nodes"),
    );
    let tunnels_v = tunnels_v?;
    // 节点列表失败不阻塞（节点名显示为空）
    let nodes = nodes_v.ok();
    let arr = tunnels_v
        .as_array()
        .ok_or("隧道列表响应格式异常")?
        .clone();
    let mut out = Vec::new();
    for t in arr {
        let tunnel_type = t
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if tunnel_type == "wol" {
            continue;
        }
        let node_id = t.get("node").and_then(|v| v.as_u64()).unwrap_or(0);
        let node_name = nodes
            .as_ref()
            .and_then(|n| n.get(&node_id.to_string()))
            .and_then(|n| n.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(NatFrpTunnel {
            id: t.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
            name: t
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            tunnel_type,
            node_id,
            node_name,
            online: t.get("online").and_then(|v| v.as_bool()).unwrap_or(false),
            banned: t.get("status").and_then(|v| v.as_i64()) == Some(2),
            status_reason: t
                .get("status_reason")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            local_ip: t
                .get("local_ip")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            local_port: t.get("local_port").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            remote: t
                .get("remote")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        });
    }
    Ok(out)
}

/// 节点列表（新建隧道选择用）
#[tauri::command]
pub async fn natfrp_list_nodes(state: State<'_, AppState>) -> Result<Vec<NatFrpNode>, String> {
    let key = require_access_key(&state)?;
    let v = crate::natfrp::api_get(&key, "/nodes").await?;
    let obj = v.as_object().ok_or("节点列表响应格式异常")?;
    let mut out = Vec::new();
    for (id_str, n) in obj {
        let Ok(id) = id_str.parse::<u64>() else { continue };
        let name = n
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let flag = n.get("flag").and_then(|v| v.as_i64()).unwrap_or(0);
        out.push(NatFrpNode {
            id,
            name: name.clone(),
            host: n
                .get("host")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            description: n
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            can_create: flag & (1 << 2) != 0,
            allow_udp: flag & (1 << 5) != 0,
            // 公开游戏专用节点：flag&2048 或名称含"多线"（官方规则）
            allow_public_game: flag & (1 << 11) != 0 || name.contains("多线"),
            offline: flag & (1 << 9) != 0,
            is_private: flag & (1 << 6) != 0,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// 创建隧道（tcp/udp 的 remote 留空 = 服务器随机分配远程端口）
#[tauri::command]
pub async fn natfrp_create_tunnel(
    state: State<'_, AppState>,
    name: String,
    tunnel_type: String,
    node_id: u64,
    local_ip: String,
    local_port: u32,
    remote: Option<String>,
    note: Option<String>,
) -> Result<u64, String> {
    let key = require_access_key(&state)?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("隧道名不能为空".into());
    }
    if !matches!(
        tunnel_type.as_str(),
        "tcp" | "udp" | "http" | "https"
    ) {
        return Err("隧道类型无效".into());
    }
    if local_ip.trim().is_empty() || local_port == 0 {
        return Err("本地 IP / 端口无效".into());
    }
    // http/https 必须绑定远程域名
    if matches!(tunnel_type.as_str(), "http" | "https") && remote.as_deref().map_or(true, |r| r.trim().is_empty())
    {
        return Err("HTTP/HTTPS 隧道必须填写绑定的子域名".into());
    }
    let mut body = serde_json::json!({
        "name": name,
        "type": tunnel_type,
        "node": node_id,
        "local_ip": local_ip.trim(),
        "local_port": local_port,
    });
    if let Some(r) = remote.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        body["remote"] = serde_json::json!(r);
    }
    if let Some(n) = note.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        body["note"] = serde_json::json!(n);
    }
    let resp = crate::natfrp::api_post_json(&key, "/tunnels", &body).await?;
    let id = resp
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or("创建成功但未返回隧道 ID")?;
    Ok(id)
}

/// 删除隧道（单条；返回是否确实被删除）
#[tauri::command]
pub async fn natfrp_delete_tunnel(
    state: State<'_, AppState>,
    tunnel_id: u64,
) -> Result<bool, String> {
    let key = require_access_key(&state)?;
    let resp = crate::natfrp::api_post_json(
        &key,
        "/tunnel/delete",
        &serde_json::json!({ "ids": tunnel_id.to_string() }),
    )
    .await?;
    let deleted = resp
        .get("deleted")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().any(|x| x.as_u64() == Some(tunnel_id)))
        .unwrap_or(false);
    Ok(deleted)
}

#[allow(dead_code)] // STOP_GRACE 供后续优雅停止扩展使用
fn _grace_hint() -> Duration {
    STOP_GRACE
}
