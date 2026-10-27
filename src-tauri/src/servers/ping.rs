// Server List Ping 协议实现（1.7+ 现代协议）
//
// 流程：SRV 解析（无端口地址先查 _minecraft._tcp）→ TCP 连接 →
// 握手包（next_state=1）→ status request → 读 JSON 状态 → ping/pong 测延迟。
// varint 帧格式手写编解码；全程 10 秒超时。
//
// MOTD 统一转换：JSON text component（1.13+）或 legacy § 字符串
// 一律转为 § 颜色码字符串，前端仅做样式渲染。

use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 单次 ping 总超时
const PING_TIMEOUT: Duration = Duration::from_secs(10);
/// 握手使用的协议版本代表值（767 = 1.21.1；服务器不校验严格匹配，仅影响个别字段展示）
const HANDSHAKE_PROTOCOL: i64 = 767;

/// ping 结果（前端展示用；motd 已转为 § 颜色码字符串）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPingResult {
    /// 延迟（毫秒，ping/pong 往返）
    pub latency_ms: u32,
    /// 版本名称（如 "1.21.1"、"Hypixel 1.21+"）
    pub version_name: String,
    pub protocol: i64,
    pub players_online: u64,
    pub players_max: u64,
    /// base64 PNG（含 "data:image/png;base64," 前缀则原样，否则补前缀）
    pub favicon: Option<String>,
    /// MOTD（§ 颜色码字符串，可能含多行 \n）
    pub motd: String,
    /// 是否为 Forge / 模组服务器（modinfo 字段）
    pub forge: bool,
    /// 在线玩家名单（players.sample；部分服务器隐藏该信息，为空表示未提供）
    pub player_sample: Vec<String>,
}

// ---------------------------------------------------------------------------
// varint 编解码
// ---------------------------------------------------------------------------

fn write_varint(buf: &mut Vec<u8>, mut v: u32) {
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if v == 0 {
            break;
        }
    }
}

/// 读取一个 varint；超过 5 字节视为协议错误
async fn read_varint(stream: &mut TcpStream) -> Result<u32, String> {
    let mut result: u32 = 0;
    for i in 0..5 {
        let byte = stream.read_u8().await.map_err(|e| format!("连接中断：{e}"))?;
        result |= ((byte & 0x7F) as u32) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err("varint 过长，协议错误".into())
}

async fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> Result<(), String> {
    let mut frame = Vec::with_capacity(payload.len() + 5);
    write_varint(&mut frame, payload.len() as u32);
    frame.extend_from_slice(payload);
    stream
        .write_all(&frame)
        .await
        .map_err(|e| format!("发送数据失败：{e}"))
}

async fn read_string(stream: &mut TcpStream) -> Result<String, String> {
    let len = read_varint(stream).await?;
    if len > 1_048_576 {
        return Err("状态数据过大，协议错误".into());
    }
    let mut buf = vec![0u8; len as usize];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("读取状态数据失败：{e}"))?;
    String::from_utf8(buf).map_err(|e| format!("状态数据不是有效 UTF-8：{e}"))
}

// ---------------------------------------------------------------------------
// SRV 解析
// ---------------------------------------------------------------------------

/// 目标地址：host + port（SRV 解析后的最终连接目标）
pub struct Target {
    pub host: String,
    pub port: u16,
}

/// 解析用户输入地址：host[:port]；无端口时先查 _minecraft._tcp SRV 记录，
/// 查询失败 / 无记录回落 host:25565
pub async fn resolve_target(address: &str) -> Result<Target, String> {
    let address = address.trim();
    let (host, port) = match address.rsplit_once(':') {
        // IPv6 字面量（[::1]:25565 或 ::1）
        Some((h, p)) if p.parse::<u16>().is_ok() && !h.is_empty() => {
            (h.trim_end_matches(':').trim_matches(['[', ']']).to_string(), p.parse::<u16>().unwrap())
        }
        _ => {
            let h = address.trim_start_matches('[').trim_end_matches(']');
            (h.to_string(), 25565u16)
        }
    };
    if host.is_empty() {
        return Err("服务器地址不能为空".into());
    }
    if port != 25565 {
        // 显式指定端口：不做 SRV
        return Ok(Target { host, port });
    }
    // SRV 查询（阻塞 DNS，放阻塞线程执行；查询失败 / 无记录回落直连 25565）
    let host2 = host.clone();
    let srv = tokio::task::spawn_blocking(move || {
        let resolver = hickory_resolver::Resolver::from_system_conf().ok()?;
        let lookup = resolver
            .srv_lookup(format!("_minecraft._tcp.{host2}"))
            .ok()?;
        lookup
            .iter()
            .min_by_key(|srv| (srv.priority(), std::cmp::Reverse(srv.weight())))
            .map(|srv| {
                (
                    srv.target().to_string().trim_end_matches('.').to_string(),
                    srv.port(),
                )
            })
    })
    .await
    .ok()
    .flatten();
    if let Some((target, srv_port)) = srv {
        return Ok(Target {
            host: target,
            port: srv_port,
        });
    }
    Ok(Target { host, port: 25565 })
}

// ---------------------------------------------------------------------------
// MOTD：JSON text component / legacy 字符串 → § 颜色码字符串
// ---------------------------------------------------------------------------

/// 颜色名 / 十六进制色 → 色码（命名色为单字符；hex 为 `#rrggbb`，
/// 由前端 `§#rrggbb` 约定渲染——现代服务端（Paper 等）MOTD 常带 hex 颜色，
/// 只映射 16 个命名色会把颜色整个丢掉）
fn color_code(name: &str) -> Option<String> {
    let named: Option<char> = match name {
        "black" => Some('0'),
        "dark_blue" => Some('1'),
        "dark_green" => Some('2'),
        "dark_aqua" => Some('3'),
        "dark_red" => Some('4'),
        "dark_purple" => Some('5'),
        "gold" => Some('6'),
        "gray" => Some('7'),
        "dark_gray" => Some('8'),
        "blue" => Some('9'),
        "green" => Some('a'),
        "aqua" => Some('b'),
        "red" => Some('c'),
        "light_purple" => Some('d'),
        "yellow" => Some('e'),
        "white" => Some('f'),
        _ => None,
    };
    if let Some(c) = named {
        return Some(c.to_string());
    }
    if name.len() == 7 && name.starts_with('#') && name[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(name.to_lowercase());
    }
    None
}

/// 段落样式（继承上下文后的有效值）
#[derive(Clone, Default)]
struct Style {
    /// 单字符 § 色码 或 `#rrggbb`（十六进制色，前端按 `§#rrggbb` 约定渲染）
    color: Option<String>,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    obfuscated: bool,
}

impl Style {
    /// 当前样式 → § 码序列（色码在前，格式码在后）
    fn codes(&self) -> String {
        let mut out = String::new();
        if let Some(c) = &self.color {
            out.push('§');
            out.push_str(c);
        }
        if self.bold {
            out.push_str("§l");
        }
        if self.italic {
            out.push_str("§o");
        }
        if self.underline {
            out.push_str("§n");
        }
        if self.strikethrough {
            out.push_str("§m");
        }
        if self.obfuscated {
            out.push_str("§k");
        }
        out
    }

    fn is_plain(&self) -> bool {
        self.color.is_none()
            && !self.bold
            && !self.italic
            && !self.underline
            && !self.strikethrough
            && !self.obfuscated
    }
}

/// 递归渲染 text component；样式变化时输出 §r 重置再重放当前样式
fn render_component(v: &Value, parent: &Style) -> String {
    match v {
        Value::String(s) => {
            // 纯字符串可能自带 § 码，原样保留
            if parent.is_plain() {
                s.clone()
            } else {
                format!("{}{}", parent.codes(), s)
            }
        }
        Value::Array(arr) => arr.iter().map(|c| render_component(c, parent)).collect(),
        Value::Object(obj) => {
            let mut style = parent.clone();
            if let Some(color) = obj.get("color").and_then(|c| c.as_str()) {
                style.color = color_code(color);
            }
            if let Some(b) = obj.get("bold").and_then(|b| b.as_bool()) {
                style.bold = b;
            }
            if let Some(b) = obj.get("italic").and_then(|b| b.as_bool()) {
                style.italic = b;
            }
            if let Some(b) = obj.get("underlined").and_then(|b| b.as_bool()) {
                style.underline = b;
            }
            if let Some(b) = obj.get("strikethrough").and_then(|b| b.as_bool()) {
                style.strikethrough = b;
            }
            if let Some(b) = obj.get("obfuscated").and_then(|b| b.as_bool()) {
                style.obfuscated = b;
            }

            let mut out = String::new();
            let text = obj.get("text").and_then(|t| t.as_str()).unwrap_or("");
            if !text.is_empty() {
                // 样式与父级一致且父级无样式时不加码，避免碎片化
                if style.is_plain() && parent.is_plain() {
                    out.push_str(text);
                } else {
                    out.push_str(&style.codes());
                    out.push_str(text);
                }
            }
            if let Some(extra) = obj.get("extra").and_then(|e| e.as_array()) {
                for child in extra {
                    out.push_str(&render_component(child, &style));
                }
            }
            out
        }
        _ => String::new(),
    }
}

/// MOTD 统一转换入口
fn motd_to_legacy(v: &Value) -> String {
    let s = render_component(v, &Style::default());
    // 清理多余的重置码开头（美观）
    s.trim_start_matches("§r").to_string()
}

// ---------------------------------------------------------------------------
// ping 主流程
// ---------------------------------------------------------------------------

/// 对地址执行完整 Server List Ping
pub async fn ping(address: &str) -> Result<ServerPingResult, String> {
    let target = resolve_target(address).await?;
    ping_target(&target.host, target.port)
        .await
        .map_err(|e| format!("{e}（{host}:{port}）", host = target.host, port = target.port))
}

async fn ping_target(host: &str, port: u16) -> Result<ServerPingResult, String> {
    let fut = async {
        let mut stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| format!("无法连接服务器：{e}"))?;

        // 握手包：id 0x00 + protocol + host + port + next_state(1)
        let mut hs = Vec::new();
        write_varint(&mut hs, 0x00);
        write_varint(&mut hs, HANDSHAKE_PROTOCOL as u32);
        let host_bytes = host.as_bytes();
        write_varint(&mut hs, host_bytes.len() as u32);
        hs.extend_from_slice(host_bytes);
        hs.extend_from_slice(&port.to_be_bytes());
        write_varint(&mut hs, 0x01);
        write_frame(&mut stream, &hs).await?;

        // status request：id 0x00（空包）
        write_frame(&mut stream, &[0x00]).await?;

        // 读状态响应：帧内首字节为 packet id（应为 0x00），其后为 JSON 字符串
        let _frame_len = read_varint(&mut stream).await?;
        let packet_id = read_varint(&mut stream).await?;
        if packet_id != 0x00 {
            return Err(format!("意外的响应类型：{packet_id:#x}"));
        }
        let json_text = read_string(&mut stream).await?;
        let status: Value =
            serde_json::from_str(&json_text).map_err(|e| format!("状态 JSON 解析失败：{e}"))?;

        // ping/pong 测延迟：id 0x01 + 8 字节时间戳
        let sent = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let mut ping = Vec::new();
        write_varint(&mut ping, 0x01);
        ping.extend_from_slice(&sent.to_be_bytes());
        write_frame(&mut stream, &ping).await?;
        let _len = read_varint(&mut stream).await?;
        let pong_id = read_varint(&mut stream).await?;
        if pong_id != 0x01 {
            return Err("ping 响应类型异常".into());
        }
        let mut echo = [0u8; 8];
        stream.read_exact(&mut echo).await.map_err(|e| format!("读取 pong 失败：{e}"))?;
        let echo_ts = i64::from_be_bytes(echo);
        let latency = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
            - echo_ts.max(sent);
        let _ = stream.shutdown().await;

        // 解析状态字段
        let version = status.get("version").cloned().unwrap_or(Value::Null);
        let players = status.get("players").cloned().unwrap_or(Value::Null);
        let favicon_raw = status
            .get("favicon")
            .and_then(|f| f.as_str())
            .map(|f| {
                if f.starts_with("data:image/") {
                    f.to_string()
                } else {
                    format!("data:image/png;base64,{f}")
                }
            });

        Ok(ServerPingResult {
            latency_ms: latency.clamp(0, u32::MAX as i64) as u32,
            version_name: version
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("未知")
                .to_string(),
            protocol: version.get("protocol").and_then(|v| v.as_i64()).unwrap_or(0),
            players_online: players
                .get("online")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            players_max: players.get("max").and_then(|v| v.as_u64()).unwrap_or(0),
            favicon: favicon_raw,
            motd: status
                .get("description")
                .map(motd_to_legacy)
                .unwrap_or_default(),
            forge: status.get("modinfo").and_then(|m| m.get("modid")).is_some(),
            // players.sample：部分服务器提供（vanilla 最多 12 个），缺失/隐藏为空
            player_sample: players
                .get("sample")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| {
                            p.get("name").and_then(|n| n.as_str()).map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    };
    tokio::time::timeout(PING_TIMEOUT, fut)
        .await
        .map_err(|_| "连接超时（10 秒无响应）".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn varint_roundtrip() {
        for v in [0u32, 1, 127, 128, 255, 2097151, 2097152, u32::MAX] {
            let mut buf = Vec::new();
            write_varint(&mut buf, v);
            // 同步验证：手动解回
            let mut expect = Vec::new();
            let mut x = v;
            loop {
                let mut b = (x & 0x7F) as u8;
                x >>= 7;
                if x != 0 {
                    b |= 0x80;
                }
                expect.push(b);
                if x == 0 {
                    break;
                }
            }
            assert_eq!(buf, expect, "varint {v} 编码");
        }
    }

    #[test]
    fn motd_legacy_string_passthrough() {
        assert_eq!(
            motd_to_legacy(&json!("§a纯字符串 MOTD")),
            "§a纯字符串 MOTD"
        );
    }

    #[test]
    fn motd_json_component_colors_and_extras() {
        let v = json!({
            "text": "Hypixel",
            "color": "gold",
            "bold": true,
            "extra": [
                { "text": " 网络游戏", "color": "yellow" },
                { "text": " §c在线", "color": "white" }
            ]
        });
        let out = motd_to_legacy(&v);
        assert!(out.contains("§6§lHypixel"), "金色加粗：{out}");
        assert!(out.contains("§e§l 网络游戏"), "颜色切换需重放样式：{out}");
        assert!(out.contains("§c在线"), "内嵌 § 码保留：{out}");
    }

    #[test]
    fn motd_style_reset_between_segments() {
        let v = json!({
            "text": "AB",
            "bold": true,
            "extra": [{ "text": "CD", "color": "red" }]
        });
        let out = motd_to_legacy(&v);
        assert!(out.contains("§lAB"), "{out}");
        assert!(out.contains("§c§lCD"), "颜色切换需重放样式：{out}");
        assert!(!out.contains("§cAB"), "AB 段不应带红色：{out}");
    }

    #[test]
    fn motd_nested_inheritance() {
        let v = json!({
            "text": "",
            "color": "aqua",
            "extra": [{ "text": "继承色", "bold": true }]
        });
        let out = motd_to_legacy(&v);
        assert!(out.contains("§b§o") == false, "不应继承出斜体：{out}");
        assert!(out.contains("§b") && out.contains("§l继承色"), "{out}");
    }
}
