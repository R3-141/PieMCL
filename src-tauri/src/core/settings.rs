// 全局设置持久化：{root}/settings.json
//
// 管理下载引擎的镜像源列表与并发上限。
// 镜像源策略：列表按序尝试（Mojang 域名改写为镜像域名），官方源恒为最后回退；
// 文件缺失或损坏时静默回落默认值（设置损坏不应阻塞启动）。

use serde::{Deserialize, Serialize};

use super::paths::LauncherPaths;

/// 默认镜像：BMCLAPI（国内快速）
pub const DEFAULT_MIRROR: &str = "https://bmclapi2.bangbang93.com";

/// 全局设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// 镜像源列表（按序尝试；仅改写 Mojang 官方域名，非官方 URL 原样下载）
    pub mirrors: Vec<String>,
    /// 下载源策略：auto（官方直连优先，失败自动降级镜像）/ official（仅官方）/ mirror（镜像优先）
    pub download_source: String,
    /// 全局并发下载连接上限
    pub global_concurrency: u32,
    /// 单主机并发上限
    pub per_host_concurrency: u32,
    /// 离线模式的默认玩家名（微软登录于账户阶段引入）
    pub default_player_name: String,
    /// 模组回收站保留天数（0 = 删除即永久移除）
    pub trash_retention_days: u32,
    /// CurseForge API Key（在线模组源；留空仅影响 CurseForge，Modrinth 不受影响）
    pub curseforge_api_key: String,
    /// 网络代理模式：none（直连）/ system（跟随系统）/ manual（手动地址）
    pub proxy_mode: String,
    /// 手动代理地址（proxy_mode = manual 时使用，如 http://127.0.0.1:7890）
    pub proxy_url: String,
    /// 自定义背景图文件名（空 = 未启用；文件位于 {root}/background/）
    pub background_image: String,
    /// 背景图模糊程度（px，0–40）
    pub background_blur: u32,
    /// 背景图遮罩暗度（%，0–70）
    pub background_dim: u32,
    /// 背景启用时的界面明暗：auto（跟随主题）/ dark / light
    pub background_theme: String,
    /// 用户手动登记的 Java 路径（java.exe 或其所在目录；自动搜索的补充，
    /// detect_all 时并入候选并探测）
    #[serde(default)]
    pub custom_java_paths: Vec<String>,
}

fn default_player_name() -> String {
    "Player".to_string()
}

fn default_trash_retention_days() -> u32 {
    7
}

fn default_download_source() -> String {
    "auto".to_string()
}

fn default_proxy_mode() -> String {
    "none".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mirrors: vec![DEFAULT_MIRROR.to_string()],
            download_source: default_download_source(),
            global_concurrency: 128,
            per_host_concurrency: 64,
            default_player_name: default_player_name(),
            trash_retention_days: default_trash_retention_days(),
            curseforge_api_key: String::new(),
            proxy_mode: default_proxy_mode(),
            proxy_url: String::new(),
            background_image: String::new(),
            background_blur: 0,
            background_dim: 25,
            background_theme: "auto".to_string(),
            custom_java_paths: Vec::new(),
        }
    }
}

impl AppSettings {
    /// 读取设置；文件缺失或解析失败时返回默认值。
    /// 兼容：检测到旧默认并发组合时一次性升级为新默认（64/16 → 128/64），
    /// 用户显式设置的其他值不受影响。
    /// （每主机并发 8 曾被作为全局默认迁移过，现仅对整合包任务在引擎内
    /// 按任务类型单独生效，全局恢复 64）
    pub async fn load(paths: &LauncherPaths) -> Self {
        let mut s: Self = match tokio::fs::read_to_string(paths.settings_path()).await {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        if s.global_concurrency == 64 && s.per_host_concurrency == 16 {
            s.global_concurrency = 128;
            s.per_host_concurrency = 64;
            let _ = s.save(paths).await; // 固化升级结果（失败无害，下次启动再试）
        } else if s.global_concurrency == 128 && s.per_host_concurrency == 8 {
            s.per_host_concurrency = 64;
            let _ = s.save(paths).await;
        }
        s
    }

    /// 校验并保存设置
    pub fn validate(&mut self) -> Result<(), String> {
        // 镜像：去空白、去尾斜杠、仅允许 http(s)，去重；空列表视为无镜像（仅官方源）
        let mut cleaned: Vec<String> = Vec::new();
        for m in &self.mirrors {
            let t = m.trim().trim_end_matches('/').to_string();
            if t.is_empty() {
                continue;
            }
            if !t.starts_with("http://") && !t.starts_with("https://") {
                return Err(format!("镜像源地址必须以 http:// 或 https:// 开头：{t}"));
            }
            if !cleaned.contains(&t) {
                cleaned.push(t);
            }
        }
        self.mirrors = cleaned;

        // 下载源策略：仅允许三档枚举
        let src = self.download_source.trim().to_string();
        if !matches!(src.as_str(), "auto" | "official" | "mirror") {
            return Err(format!("未知的下载源策略：{src}"));
        }
        self.download_source = src;

        if !(1..=256).contains(&self.global_concurrency) {
            return Err("全局并发上限必须在 1–256 之间".to_string());
        }
        if !(1..=64).contains(&self.per_host_concurrency) {
            return Err("单主机并发上限必须在 1–64 之间".to_string());
        }
        // 玩家名：去空白，空则回落默认
        let name = self.default_player_name.trim().to_string();
        self.default_player_name = if name.is_empty() { default_player_name() } else { name };
        // 回收站保留期：0–3650 天（0 = 立即永久删除）
        self.trash_retention_days = self.trash_retention_days.min(3650);
        // CurseForge API Key：去首尾空白
        self.curseforge_api_key = self.curseforge_api_key.trim().to_string();

        // 网络代理：模式枚举校验；手动模式要求地址非空且协议合法
        let pm = self.proxy_mode.trim().to_string();
        if !matches!(pm.as_str(), "none" | "system" | "manual") {
            return Err(format!("未知的网络代理模式：{pm}"));
        }
        self.proxy_mode = pm;
        let pu = self.proxy_url.trim().to_string();
        if self.proxy_mode == "manual" {
            if pu.is_empty()
                || !(pu.starts_with("http://")
                    || pu.starts_with("https://")
                    || pu.starts_with("socks5://"))
            {
                return Err(
                    "手动代理地址需以 http://、https:// 或 socks5:// 开头".to_string(),
                );
            }
        }
        self.proxy_url = pu;

        // 自定义背景：文件名仅保留基名（防路径穿越）；模糊与暗度限幅
        self.background_image = self
            .background_image
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        self.background_blur = self.background_blur.min(40);
        self.background_dim = self.background_dim.min(70);
        // 背景界面明暗：枚举校验
        let bt = self.background_theme.trim().to_string();
        if !matches!(bt.as_str(), "auto" | "dark" | "light") {
            return Err(format!("未知的背景界面配色：{bt}"));
        }
        self.background_theme = bt;

        // 手动登记的 Java 路径：去空白去重，上限 16 条
        let mut paths: Vec<String> = Vec::new();
        for p in &self.custom_java_paths {
            let t = p.trim().to_string();
            if !t.is_empty() && !paths.contains(&t) {
                paths.push(t);
            }
        }
        paths.truncate(16);
        self.custom_java_paths = paths;
        Ok(())
    }

    /// 保存到 settings.json（原子性要求不高，直接覆写）
    pub async fn save(&self, paths: &LauncherPaths) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| format!("序列化设置失败：{e}"))?;
        tokio::fs::write(paths.settings_path(), text)
            .await
            .map_err(|e| format!("保存设置失败：{e}"))
    }
}
