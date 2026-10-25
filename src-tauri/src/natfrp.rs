// 樱花内网穿透（natfrp.com）支持：配置持久化 + frpc 下载解析
//
// - 配置：{启动器目录}/natfrp/config.json（访问密钥 + frpc 路径，明文本机存储）
// - frpc：樱花定制版 frpc，启动命令 `frpc -f <访问密钥>:<隧道ID>`（无需配置文件）。
//   自动下载：请求樱花客户端清单 API 解析 Windows amd64 直链（API 变动时回落
//   页面「手动指定 frpc.exe」）；下载产物兼容裸 exe（MZ 魔数）与 zip 包两种形式。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::paths::LauncherPaths;

/// 樱花客户端清单 API（返回各平台 frpc 版本与下载直链）
const CLIENTS_API: &str = "https://api.natfrp.com/v4/system/clients";

/// 樱花隧道列表 API（按访问密钥拉取当前用户全部隧道的连接信息）
const TUNNELS_API: &str = "https://api.natfrp.com/v4/tunnels";

/// 樱花 API v4 根（隧道管理 / 节点列表等）
const API_BASE: &str = "https://api.natfrp.com/v4";

/// API 不可用时的内置兜底直链（与官方 clients 清单的 frpc/windows_amd64 保持同步）
const FALLBACK_FRPC_URL: &str =
    "https://nya.globalslb.net/natfrp/client/frpc/0.51.0-sakura-15.1/frpc_windows_amd64.exe";
const FALLBACK_FRPC_SIZE: u64 = 14_209_024;

/// frpc 下载信息
pub struct FrpcDownload {
    pub urls: Vec<String>,
    pub size: Option<u64>,
}

/// 联机配置（config.json）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NatFrpConfig {
    /// 访问密钥（natfrp.com 面板「访问密钥」页）
    pub access_key: String,
    /// 隧道 ID（樱花面板隧道列表中的数字 ID，持久化避免重启后重填）
    pub tunnel_id: String,
    /// frpc 可执行文件路径（None = 使用默认路径 {natfrp}/frpc.exe）
    pub frpc_path: Option<String>,
}

impl NatFrpConfig {
    pub fn load(paths: &LauncherPaths) -> Self {
        let Ok(text) = std::fs::read_to_string(paths.natfrp_config()) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self, paths: &LauncherPaths) -> Result<(), String> {
        if let Some(parent) = paths.natfrp_dir().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::create_dir_all(paths.natfrp_dir());
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化联机配置失败：{e}"))?;
        std::fs::write(paths.natfrp_config(), text)
            .map_err(|e| format!("保存联机配置失败：{e}"))
    }

    /// frpc 可执行文件路径：优先用户指定的，其次默认路径
    pub fn frpc_path(&self, paths: &LauncherPaths) -> PathBuf {
        self.frpc_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| paths.natfrp_frpc_path())
    }

    /// frpc 是否已就绪（配置路径或默认路径存在且为文件）
    pub fn frpc_ready(&self, paths: &LauncherPaths) -> bool {
        self.frpc_path(paths).is_file()
    }
}

/// 通过樱花 API 解析 frpc（Windows amd64）下载信息。
/// 精准路径 root.frpc.archs.windows_amd64（清单实测结构）；API 失败或结构
/// 变动时回落内置兜底直链，保证下载流程可用。
pub async fn resolve_frpc_download() -> FrpcDownload {
    let http = reqwest::Client::builder()
        .user_agent("PieMCL/0.1")
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build();
    if let Ok(http) = http {
        if let Ok(resp) = http.get(CLIENTS_API).send().await {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                let url = v
                    .pointer("/frpc/archs/windows_amd64/url")
                    .and_then(|x| x.as_str())
                    .map(String::from);
                if let Some(url) = url {
                    let size = v
                        .pointer("/frpc/archs/windows_amd64/size")
                        .and_then(|x| x.as_u64());
                    return FrpcDownload {
                        urls: vec![url],
                        size,
                    };
                }
            }
        }
    }
    FrpcDownload {
        urls: vec![FALLBACK_FRPC_URL.to_string()],
        size: Some(FALLBACK_FRPC_SIZE),
    }
}

/// 从下载产物中提取 frpc.exe：裸 exe（MZ 魔数）直接使用；zip 包（PK 魔数）
/// 解压取 frpc*.exe / 唯一 exe。返回落位路径。
pub fn extract_frpc(downloaded: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    let data = std::fs::read(downloaded).map_err(|e| format!("读取下载产物失败：{e}"))?;
    if data.len() < 2 {
        return Err("下载产物为空".into());
    }
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("创建目录失败：{e}"))?;
    let dest = dest_dir.join("frpc.exe");

    if &data[..2] == b"MZ" {
        std::fs::write(&dest, &data).map_err(|e| format!("写入 frpc.exe 失败：{e}"))?;
        return Ok(dest);
    }
    if &data[..2] == b"PK" {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&data))
            .map_err(|e| format!("打开压缩包失败：{e}"))?;
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| format!("读取压缩包条目失败：{e}"))?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_lowercase();
            if name.ends_with(".exe") && (name.contains("frpc") || name.contains("frp")) {
                let mut out = Vec::with_capacity(entry.size() as usize);
                std::io::Read::read_to_end(&mut entry, &mut out)
                    .map_err(|e| format!("解压失败：{e}"))?;
                if out.len() < 2 || &out[..2] != b"MZ" {
                    continue; // 非 exe 内容，跳过
                }
                std::fs::write(&dest, &out).map_err(|e| format!("写入 frpc.exe 失败：{e}"))?;
                return Ok(dest);
            }
        }
        return Err("压缩包内未找到 frpc 可执行文件".into());
    }
    Err("下载产物既不是 exe 也不是 zip，可能下载链接已失效".into())
}

// ---------------------------------------------------------------------------
// 日志连接地址提取
// ---------------------------------------------------------------------------

/// 从一行 frpc 日志提取樱花隧道连接地址（`host:端口`）。
/// 两种来源：① 樱花日志特有的 `>>host:port<<` 定界形式（域名地址与节点 IP 均走此格式）；
/// ② `.natfrp.*` 域名兜底（.natfrp.com / .natfrp.cloud 等）。
/// 手写扫描（项目未引入 regex）。
pub fn extract_addresses(line: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    extract_bracket_addrs(line, &mut found);
    extract_natfrp_addrs(line, &mut found);
    let mut out: Vec<String> = Vec::new();
    for a in found {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// 提取 `>>host:port<<` 形式的地址（樱花 frpc 日志：
/// 「使用 >>frp-top.com:62038<< 连接你的隧道」「或使用节点 IP 连接 >>103.91.208.133:62038<<」）
fn extract_bracket_addrs(line: &str, out: &mut Vec<String>) {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(">>") {
        let host_start = search_from + rel + 2;
        let mut cursor = host_start;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric()
                || bytes[cursor] == b'.'
                || bytes[cursor] == b'-')
        {
            cursor += 1;
        }
        if cursor > host_start
            && cursor < bytes.len()
            && bytes[cursor] == b':'
        {
            let port_start = cursor + 1;
            let mut port_end = port_start;
            while port_end < bytes.len() && bytes[port_end].is_ascii_digit() {
                port_end += 1;
            }
            // 端口后必须紧跟 "<<" 才认定界地址；端口合法性粗筛（1–65535）
            if port_end > port_start
                && port_end + 1 < bytes.len()
                && bytes[port_end] == b'<'
                && bytes[port_end + 1] == b'<'
                && lower[host_start..cursor]
                    .starts_with(|c: char| c.is_ascii_alphanumeric())
            {
                if let Ok(p) = lower[port_start..port_end].parse::<u16>() {
                    if p > 0 {
                        out.push(format!("{}:{}", &lower[host_start..cursor], p));
                    }
                }
            }
        }
        search_from = host_start.max(search_from + 2);
    }
}

/// 提取 `.natfrp.*` 域名形式的地址（兜底：日志其他形式提及 natfrp 域名时）
fn extract_natfrp_addrs(line: &str, out: &mut Vec<String>) {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut search_from = 0usize;
    while let Some(rel) = lower[search_from..].find(".natfrp.") {
        let dot_pos = search_from + rel; // ".natfrp." 的起始 '.' 位置

        // 向前扫域名主体（字母数字点连字符），得到完整 host
        let mut start = dot_pos;
        while start > 0 {
            let c = bytes[start - 1];
            if c.is_ascii_alphanumeric() || c == b'.' || c == b'-' {
                start -= 1;
            } else {
                break;
            }
        }
        // 向后越过 ".natfrp.xxx" 找端口分隔 ':'
        let mut cursor = dot_pos + ".natfrp.".len();
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'.' || bytes[cursor] == b'-')
        {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b':' {
            let port_start = cursor + 1;
            let mut port_end = port_start;
            while port_end < bytes.len() && bytes[port_end].is_ascii_digit() {
                port_end += 1;
            }
            if port_end > port_start {
                let host = &lower[start..cursor];
                let port = &lower[port_start..port_end];
                // 端口合法性粗筛（1–65535）
                if let Ok(p) = port.parse::<u16>() {
                    if p > 0 && !host.starts_with('.') && !host.ends_with('.') {
                        out.push(format!("{host}:{port}"));
                    }
                }
            }
        }
        search_from = cursor.max(dot_pos + 1);
    }
}

/// 对 host 做 A / AAAA 解析，返回 (IPv4 列表, IPv6 列表)；查询失败返回空表
pub async fn resolve_host_ips(host: &str) -> (Vec<String>, Vec<String>) {
    use hickory_resolver::proto::rr::RData;
    let host = host.to_string();
    tokio::task::spawn_blocking(move || {
        let Ok(resolver) = hickory_resolver::Resolver::from_system_conf() else {
            return (Vec::new(), Vec::new());
        };
        let v4 = resolver
            .lookup(&host, hickory_resolver::proto::rr::RecordType::A)
            .map(|l| {
                l.iter()
                    .filter_map(|r| match r {
                        RData::A(a) => Some(a.0.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let v6 = resolver
            .lookup(&host, hickory_resolver::proto::rr::RecordType::AAAA)
            .map(|l| {
                l.iter()
                    .filter_map(|r| match r {
                        RData::AAAA(a) => Some(a.0.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        (v4, v6)
    })
    .await
    .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 隧道连接地址（隧道列表 API）
// ---------------------------------------------------------------------------

/// 递归遍历隧道 JSON 的全部字符串值，提取 `*.natfrp.*:端口` 连接地址。
/// 隧道 API 的字段命名随版本演进可能变动（remote_addr / address / node 等），
/// 因此不赌具体字段名，宽容扫描任意层级的字符串值。
fn collect_tunnel_addresses(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            for addr in extract_addresses(s) {
                if !out.contains(&addr) {
                    out.push(addr);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_tunnel_addresses(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, val) in map {
                collect_tunnel_addresses(val, out);
            }
        }
        _ => {}
    }
}

/// 按访问密钥拉取隧道列表并解析全部连接地址（`host:端口`）。
/// 密钥无效返回带服务端提示的错误；网络/解析失败返回错误由前端提示。
pub async fn fetch_tunnel_addrs(access_key: &str) -> Result<Vec<String>, String> {
    let http = reqwest::Client::builder()
        .user_agent("PieMCL/0.1")
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;
    let resp = http
        .get(TUNNELS_API)
        .query(&[("token", access_key)])
        .send()
        .await
        .map_err(|e| format!("连接樱花隧道 API 失败：{e}"))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析隧道 API 响应失败：{e}"))?;
    // 业务错误码：401=密钥无效等，优先透出服务端 msg
    if let Some(code) = v.get("code").and_then(|c| c.as_i64()) {
        if code != 200 {
            let msg = v
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            return Err(format!("拉取隧道信息失败：{msg}（错误码 {code}）"));
        }
    }
    let mut out = Vec::new();
    collect_tunnel_addresses(&v, &mut out);
    Ok(out)
}

// ---------------------------------------------------------------------------
// 樱花 API v4 通用请求（隧道管理 / 节点列表）
// ---------------------------------------------------------------------------

fn api_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("PieMCL/0.1")
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))
}

/// 统一的业务错误判定：响应含 `code` 且 != 200 时透出 msg（401 特化为密钥提示）
fn check_api_code(v: &serde_json::Value) -> Result<(), String> {
    let Some(code) = v.get("code").and_then(|c| c.as_i64()) else {
        return Ok(()); // 成功响应（数组 / 无 code 字段）
    };
    if code == 200 {
        return Ok(());
    }
    let msg = v.get("msg").and_then(|m| m.as_str()).unwrap_or("未知错误");
    if code == 401 {
        return Err("访问密钥无效，请在联机页检查配置".into());
    }
    Err(format!("樱花接口错误：{msg}（错误码 {code}）"))
}

/// GET {API_BASE}{path}?token=密钥
pub async fn api_get(token: &str, path: &str) -> Result<serde_json::Value, String> {
    let http = api_client()?;
    let resp = http
        .get(format!("{API_BASE}{path}"))
        .query(&[("token", token)])
        .send()
        .await
        .map_err(|e| format!("连接樱花 API 失败：{e}"))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析樱花 API 响应失败：{e}"))?;
    check_api_code(&v)?;
    Ok(v)
}

/// POST {API_BASE}{path}?token=密钥（JSON body）
pub async fn api_post_json(
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let http = api_client()?;
    let resp = http
        .post(format!("{API_BASE}{path}"))
        .query(&[("token", token)])
        .json(body)
        .send()
        .await
        .map_err(|e| format!("连接樱花 API 失败：{e}"))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析樱花 API 响应失败：{e}"))?;
    check_api_code(&v)?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_natfrp_addresses() {
        let line = "[SakuraFrp] 隧道 #123 已连接: tcp://cn-cd-gp-1.natfrp.cloud:45000 -> 127.0.0.1:25565";
        assert_eq!(extract_addresses(line), vec!["cn-cd-gp-1.natfrp.cloud:45000".to_string()]);
        // 无 natfrp 域名的行不提取（本地地址噪音）
        assert!(extract_addresses("listen on 0.0.0.0:25565").is_empty());
        assert!(extract_addresses("connect to 127.0.0.1:7000").is_empty());
    }

    #[test]
    fn extract_bracket_addresses() {
        // 樱花 frpc 实际日志（实测截图原文）：域名地址与节点 IP 均走 >>host:port<< 定界
        let l1 = "使用 >>frp-top.com:62038<< 连接你的隧道";
        assert_eq!(extract_addresses(l1), vec!["frp-top.com:62038".to_string()]);
        let l2 = "或使用节点 IP 连接（不推荐）>>103.91.208.133:62038<<";
        assert_eq!(
            extract_addresses(l2),
            vec!["103.91.208.133:62038".to_string()]
        );
        // 非法形式不提取：缺 "<<"、端口缺失、host 为空
        assert!(extract_addresses("使用 >>frp-top.com:62038 连接").is_empty());
        assert!(extract_addresses("使用 >>:62038<< 连接").is_empty());
    }

    #[test]
    fn extract_multiple_and_dedupe() {
        let line = "a.natfrp.cloud:1000 | b.natfrp.cloud:2000 | a.natfrp.cloud:1000";
        let out = extract_addresses(line);
        assert_eq!(
            out,
            vec![
                "a.natfrp.cloud:1000".to_string(),
                "b.natfrp.cloud:2000".to_string()
            ]
        );
    }

    #[test]
    fn extract_requires_valid_port() {
        // 端口缺失 / 非法 → 不收集
        assert!(extract_addresses("x.natfrp.cloud:").is_empty());
        assert!(extract_addresses("x.natfrp.cloud:abc").is_empty());
        assert!(extract_addresses("x.natfrp.cloud:99999").is_empty());
    }

    #[test]
    fn tunnel_addresses_tolerant_scan() {
        // 不赌字段名：任意层级的字符串值均可提取
        let v = json!({
            "code": 200,
            "data": {
                "list": [
                    { "remote_addr": "cn-cd-gp-1.natfrp.cloud:45000" },
                    { "node": { "address": "cn-hk-2.natfrp.cloud:39001" } },
                    { "irrelevant": "https://api.natfrp.com/v4" }
                ]
            }
        });
        let mut out = Vec::new();
        collect_tunnel_addresses(&v, &mut out);
        assert_eq!(
            out,
            vec![
                "cn-cd-gp-1.natfrp.cloud:45000".to_string(),
                "cn-hk-2.natfrp.cloud:39001".to_string()
            ]
        );
    }

    #[test]
    fn extract_frpc_from_raw_exe() {
        let base = std::env::temp_dir().join(format!("piemcl-frpc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let downloaded = base.join("bin");
        std::fs::write(&downloaded, b"MZ-fake-exe").unwrap();
        let dest = extract_frpc(&downloaded, &base.join("out")).unwrap();
        assert_eq!(dest, base.join("out").join("frpc.exe"));
        assert!(dest.is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn extract_frpc_from_zip() {
        let base = std::env::temp_dir().join(format!("piemcl-frpcz-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // 构造含 frpc.exe（MZ）的 zip
        let buf = {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            w.start_file("frpc.exe", zip::write::SimpleFileOptions::default()).unwrap();
            use std::io::Write;
            w.write_all(b"MZ-zip-exe").unwrap();
            w.finish().unwrap().into_inner()
        };
        let downloaded = base.join("bin");
        std::fs::write(&downloaded, &buf).unwrap();
        let dest = extract_frpc(&downloaded, &base.join("out")).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"MZ-zip-exe");
        let _ = std::fs::remove_dir_all(&base);
    }
}
