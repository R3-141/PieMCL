// 离线皮肤注入（批次 D）：本地 Yggdrasil 会话服务 + authlib-injector jar 管理
//
// 原理：authlib-injector 以 -javaagent 注入游戏，把 authlib 的会话服务器地址
// 重定向到 `http://127.0.0.1:{port}`；本服务按 Yggdrasil 协议返回离线玩家的
// textures property（皮肤贴图由本地提供），实现离线模式穿正版皮肤。
//
// 服务生命周期：每次启动离线实例（且有皮肤）时起一个随机端口实例，
// 游戏进程退出后由启动编排关闭（LocalYggHandle::shutdown）。
// 签名说明：authlib-injector 对本地服务器（127.0.0.1）自动跳过 textures 签名校验，
// 因此本服务返回 unsigned 响应即可。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine;
use serde_json::{Value, json};

/// authlib-injector 版本（GitHub release 固定版本）
const AGENT_VERSION: &str = "1.2.5";
/// jar 合理大小下限（防镜像返回 HTML 错误页）
const AGENT_MIN_SIZE: u64 = 100_000;

/// 注入的皮肤数据（服务状态）
struct LocalSkin {
    /// 无连字符 UUID
    uuid_nodash: String,
    player_name: String,
    /// classic / slim
    variant: String,
    png: Vec<u8>,
    /// http://127.0.0.1:{port}（textures url 前缀）
    base_url: String,
}

/// 本地服务句柄（shutdown 即关闭）
pub struct LocalYggHandle {
    pub port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl LocalYggHandle {
    /// 关闭服务（游戏进程退出时调用）
    pub fn shutdown(self) {
        self.task.abort();
        eprintln!("[local-ygg] 皮肤服务已关闭");
    }
}

/// 启动本地 Yggdrasil 会话服务（绑定 127.0.0.1 随机端口）
pub async fn start(
    uuid_nodash: String,
    player_name: String,
    variant: String,
    png: Vec<u8>,
) -> Result<LocalYggHandle, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("本地皮肤服务绑定端口失败：{e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("获取本地端口失败：{e}"))?
        .port();
    let state = Arc::new(LocalSkin {
        uuid_nodash,
        player_name,
        variant,
        png,
        base_url: format!("http://127.0.0.1:{port}"),
    });
    eprintln!(
        "[local-ygg] 皮肤服务已启动：127.0.0.1:{port}（{} · {}）",
        state.player_name, state.variant
    );

    let app = Router::new()
        .route("/", get(metadata))
        .route(
            "/sessionserver/session/minecraft/profile/{uuid}",
            get(profile),
        )
        .route("/textures/{name}", get(texture))
        .with_state(state);

    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[local-ygg] 服务异常退出：{e}");
        }
    });

    Ok(LocalYggHandle { port, task })
}

/// 元数据端点（authlib-injector 初始化时请求；本地服务器无需签名公钥）
async fn metadata() -> Json<Value> {
    Json(json!({
        "meta": {
            "serverName": "PieMCL Local Skin Server",
            "implementationName": "PieMCL",
            "implementationVersion": env!("CARGO_PKG_VERSION"),
        },
        "skinDomains": ["127.0.0.1", "localhost"],
    }))
}

/// profile 端点：返回带 textures property 的玩家资料（unsigned）
async fn profile(
    State(state): State<Arc<LocalSkin>>,
    AxumPath(uuid): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    // 规范化比较（忽略连字符与大小写；authlib 以无连字符形式请求）
    let requested: String = uuid.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let requested = requested.to_ascii_lowercase();
    if requested != state.uuid_nodash.to_ascii_lowercase() {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut skin_entry = json!({
        "url": format!("{}/textures/skin", state.base_url),
    });
    // 纤细模型经 SKIN.meta.model 声明（缺省 classic）
    if state.variant == "slim" {
        skin_entry["meta"] = json!({ "model": "slim" });
    }
    let textures = json!({
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        "profileId": state.uuid_nodash,
        "profileName": state.player_name,
        "isPublic": true,
        "textures": { "SKIN": skin_entry },
    });
    let value = base64::engine::general_purpose::STANDARD.encode(textures.to_string());
    Ok(Json(json!({
        "id": state.uuid_nodash,
        "name": state.player_name,
        "properties": [
            { "name": "textures", "value": value }
        ],
    })))
}

/// 皮肤贴图端点：返回 PNG 二进制
async fn texture(
    State(state): State<Arc<LocalSkin>>,
    AxumPath(_name): AxumPath<String>,
) -> (StatusCode, [(axum::http::header::HeaderName, String); 1], Vec<u8>) {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/png".to_string())],
        state.png.clone(),
    )
}

// ---------------------------------------------------------------------------
// authlib-injector jar 管理（ghproxy 类镜像优先 + GitHub 直连兜底）
// ---------------------------------------------------------------------------

const GITHUB_URL: &str = concat!(
    "https://github.com/yushijinhun/authlib-injector/releases/download/v",
    "1.2.5",
    "/authlib-injector-1.2.5.jar"
);

/// jar 缓存路径：{cache_dir}/authlib-injector/authlib-injector-{ver}.jar
pub fn agent_jar_path(cache_dir: &Path) -> PathBuf {
    cache_dir
        .join("authlib-injector")
        .join(format!("authlib-injector-{AGENT_VERSION}.jar"))
}

/// 确保注入 agent jar 存在（缺失或损坏时自动下载），返回 jar 路径
pub async fn ensure_agent_jar(cache_dir: &Path) -> Result<PathBuf, String> {
    let dest = agent_jar_path(cache_dir);
    // 已存在且大小合理 → 直接复用
    if let Ok(meta) = tokio::fs::metadata(&dest).await {
        if meta.len() > AGENT_MIN_SIZE {
            return Ok(dest);
        }
    }
    tokio::fs::create_dir_all(dest.parent().unwrap())
        .await
        .map_err(|e| format!("创建缓存目录失败：{e}"))?;

    // 镜像候选：ghproxy 类加速优先，GitHub 直连兜底
    let candidates = [
        format!("https://ghfast.top/{GITHUB_URL}"),
        format!("https://gh-proxy.com/{GITHUB_URL}"),
        GITHUB_URL.to_string(),
    ];
    let mut last_err = String::new();
    for url in &candidates {
        eprintln!("[local-ygg] 下载 authlib-injector：{url}");
        match download_jar(url).await {
            Ok(bytes) => {
                tokio::fs::write(&dest, &bytes)
                    .await
                    .map_err(|e| format!("保存 agent jar 失败：{e}"))?;
                eprintln!("[local-ygg] agent jar 下载成功（{} 字节）", bytes.len());
                return Ok(dest);
            }
            Err(e) => {
                eprintln!("[local-ygg] 该源下载失败：{e}");
                last_err = e;
            }
        }
    }
    Err(format!(
        "下载 authlib-injector 失败（镜像与直连均不可用）：{last_err}"
    ))
}

async fn download_jar(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("PieMCL/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("{e}"))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取下载内容失败：{e}"))?
        .to_vec();
    // 校验：大小合理 + zip 魔数（PK）
    if (bytes.len() as u64) < AGENT_MIN_SIZE {
        return Err(format!("文件过小（{} 字节），疑似错误页面", bytes.len()));
    }
    if bytes.len() < 2 || &bytes[..2] != b"PK" {
        return Err("下载内容不是有效的 jar 文件".into());
    }
    Ok(bytes)
}
