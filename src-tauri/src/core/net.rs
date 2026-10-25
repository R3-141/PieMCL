// 全局网络配置：HTTP 代理
//
// 代理模式（settings.proxy_mode）：
// - none：显式直连（no_proxy，覆盖环境变量中的残留代理设置——历史默认行为）
// - system：跟随系统代理与环境变量（reqwest 默认行为，依赖 system-proxy feature）
// - manual：手动指定代理地址（settings.proxy_url，http/https/socks5）
//
// 收口：所有 reqwest::Client 构造必须经 apply_proxy 包装，
// 保证代理设置对下载引擎、Modrinth/CurseForge、OptiFine、微软登录等全链路生效。

use std::sync::RwLock;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProxyConfig {
    /// none / system / manual
    pub mode: String,
    /// manual 模式的代理地址（如 http://127.0.0.1:7890）
    pub url: String,
}

static PROXY: RwLock<Option<ProxyConfig>> = RwLock::new(None);

/// 启动 / 保存设置时更新全局代理配置（Client 每次请求时新建的场景立即生效；
/// 下载引擎的常驻 Client 需重启生效）
pub fn init_proxy(cfg: ProxyConfig) {
    if let Ok(mut guard) = PROXY.write() {
        *guard = Some(cfg);
    }
}

/// 统一 HTTP 客户端构造收口：按全局代理配置附加代理层
pub fn apply_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    let Some(cfg) = current_proxy() else {
        return builder.no_proxy();
    };
    match cfg.mode.as_str() {
        "manual" => {
            let url = cfg.url.trim().to_string();
            if url.is_empty() {
                return builder.no_proxy();
            }
            match reqwest::Proxy::all(&url) {
                Ok(px) => builder.proxy(px),
                // 代理地址非法时回落直连，避免整体不可用
                Err(_) => builder.no_proxy(),
            }
        }
        "system" => builder,
        _ => builder.no_proxy(),
    }
}

fn current_proxy() -> Option<ProxyConfig> {
    PROXY.read().ok().and_then(|g| g.clone())
}
