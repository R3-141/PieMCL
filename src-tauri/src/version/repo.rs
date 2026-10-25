// 版本清单 / 版本 JSON 仓库
//
// 下载源策略：BMCLAPI 镜像优先，Mojang 官方备用回退。
// 清单带 24h 本地缓存（sidecar 记录获取时间与来源）；
// 版本 JSON 永久缓存于 cache/versions/{id}/{id}.json。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::core::error::{AppError, AppResult};
use crate::core::paths::LauncherPaths;
use super::model::{ManifestMeta, McVersionEntry, VersionManifest};

/// BMCLAPI 镜像根
const BMCLAPI_BASE: &str = "https://bmclapi2.bangbang93.com";
/// Mojang 官方清单地址（备用）
const MOJANG_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
/// BMCLAPI 清单地址（首选）
const BMCLAPI_MANIFEST_URL: &str =
    "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json";

/// 清单缓存有效期：24 小时
const MANIFEST_TTL_SECS: i64 = 24 * 60 * 60;

/// 把 Mojang 官方 URL 改写为 BMCLAPI 镜像 URL（复用公共镜像规则）
fn mirror_url(url: &str) -> String {
    crate::download::mirror::rewrite_with_mirror(BMCLAPI_BASE, url)
        .unwrap_or_else(|| url.to_string())
}

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 版本仓库：清单与版本 JSON 的获取、缓存
#[derive(Clone)]
pub struct VersionRepo {
    http: reqwest::Client,
    paths: LauncherPaths,
}

impl VersionRepo {
    pub fn new(paths: LauncherPaths) -> Self {
        let http = crate::core::net::apply_proxy(reqwest::Client::builder()
            .user_agent("PieMCL/0.1")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30)))
            .build()
            .expect("构建 HTTP 客户端失败");
        Self { http, paths }
    }

    /// 读取缓存清单（不论是否过期）
    async fn read_cached_manifest(&self) -> Option<VersionManifest> {
        let text = tokio::fs::read_to_string(self.paths.manifest_cache())
            .await
            .ok()?;
        serde_json::from_str(&text).ok()
    }

    /// 读取清单缓存元信息
    async fn read_manifest_meta(&self) -> Option<ManifestMeta> {
        let text = tokio::fs::read_to_string(self.paths.manifest_meta())
            .await
            .ok()?;
        serde_json::from_str(&text).ok()
    }

    /// 从指定 URL 拉取清单并写缓存
    async fn fetch_manifest_from(&self, url: &str, source: &str) -> AppResult<VersionManifest> {
        let manifest: VersionManifest = self.http.get(url).send().await?.json().await?;
        // 写缓存（失败不致命，仅影响下次启动速度）
        if let Ok(text) = serde_json::to_string_pretty(&manifest) {
            let _ = tokio::fs::write(self.paths.manifest_cache(), text).await;
            let meta = ManifestMeta {
                fetched_at: now_epoch(),
                source: source.to_string(),
            };
            if let Ok(mt) = serde_json::to_string_pretty(&meta) {
                let _ = tokio::fs::write(self.paths.manifest_meta(), mt).await;
            }
        }
        Ok(manifest)
    }

    /// 获取版本清单：缓存新鲜则直接用；否则 BMCLAPI 优先、官方备用；
    /// 双源均失败时回退旧缓存。
    pub async fn fetch_manifest(&self, refresh: bool) -> AppResult<VersionManifest> {
        if !refresh {
            if let Some(meta) = self.read_manifest_meta().await {
                if now_epoch() - meta.fetched_at < MANIFEST_TTL_SECS {
                    if let Some(m) = self.read_cached_manifest().await {
                        return Ok(m);
                    }
                }
            }
        }

        let mut last_err: Option<AppError> = None;
        for (url, source) in [
            (BMCLAPI_MANIFEST_URL, "BMCLAPI"),
            (MOJANG_MANIFEST_URL, "Mojang"),
        ] {
            match self.fetch_manifest_from(url, source).await {
                Ok(m) => return Ok(m),
                Err(e) => last_err = Some(e),
            }
        }

        // 双源失败 → 回退旧缓存
        if let Some(m) = self.read_cached_manifest().await {
            return Ok(m);
        }
        Err(last_err.unwrap_or_else(|| {
            AppError::msg("无法获取版本清单：BMCLAPI 与官方源均不可用，且本地无缓存")
        }))
    }

    /// 按 id 获取版本 JSON：缓存优先，缺失时按清单 URL 下载（镜像优先）
    pub async fn fetch_version_json_by_id(&self, id: &str) -> AppResult<Value> {
        let cache_file = self.paths.version_json_cache(id);
        if let Ok(text) = tokio::fs::read_to_string(&cache_file).await {
            return Ok(serde_json::from_str(&text)?);
        }

        let manifest = self.fetch_manifest(false).await?;
        let entry = manifest
            .versions
            .iter()
            .find(|v| v.id == id)
            .ok_or_else(|| AppError::msg(format!("版本清单中找不到版本 {id}")))?;

        let mut last_err: Option<AppError> = None;
        for url in [mirror_url(&entry.url), entry.url.clone()] {
            match self.http.get(&url).send().await {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(json) => {
                        if let Some(parent) = cache_file.parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }
                        if let Ok(text) = serde_json::to_string_pretty(&json) {
                            let _ = tokio::fs::write(&cache_file, text).await;
                        }
                        return Ok(json);
                    }
                    Err(e) => last_err = Some(e.into()),
                },
                Err(e) => last_err = Some(e.into()),
            }
        }
        Err(last_err.unwrap_or_else(|| AppError::msg(format!("下载版本 {id} 的元数据失败"))))
    }

    /// 列出可用 MC 版本（供前端创建实例时选择）
    pub async fn list_mc_versions(
        &self,
        show_snapshots: bool,
        refresh: bool,
    ) -> AppResult<Vec<McVersionEntry>> {
        let manifest = self.fetch_manifest(refresh).await?;
        Ok(manifest
            .versions
            .iter()
            .filter(|v| show_snapshots || v.version_type == "release")
            .map(McVersionEntry::from)
            .collect())
    }

    /// 拉取 Fabric / Quilt 的 profile JSON
    /// （标准 version JSON 结构，inheritsFrom 指向 MC 版本；BMCLAPI 优先、官方 meta 备用）
    pub async fn fetch_loader_profile(
        &self,
        loader_type: &str,
        mc_version: &str,
        loader_version: &str,
    ) -> AppResult<Value> {
        let urls: Vec<String> = match loader_type {
            "fabric" => vec![
                format!("{BMCLAPI_BASE}/fabric-meta/v2/versions/loader/{mc_version}/{loader_version}/profile/json"),
                format!("https://meta.fabricmc.net/v2/versions/loader/{mc_version}/{loader_version}/profile/json"),
            ],
            "quilt" => vec![
                format!("{BMCLAPI_BASE}/quilt-meta/v3/versions/loader/{mc_version}/{loader_version}/profile/json"),
                format!("https://meta.quiltmc.org/v3/versions/loader/{mc_version}/{loader_version}/profile/json"),
            ],
            other => {
                return Err(AppError::msg(format!(
                    "{other} 不提供 profile JSON（Forge / NeoForge 走独立的 install_profile 流程，暂不支持）"
                )))
            }
        };

        let mut last_err: Option<AppError> = None;
        for url in urls {
            match self.http.get(&url).send().await {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(json) => return Ok(json),
                    Err(e) => last_err = Some(e.into()),
                },
                Err(e) => last_err = Some(e.into()),
            }
        }
        Err(last_err
            .unwrap_or_else(|| AppError::msg(format!("获取 {loader_type} 加载器配置失败"))))
    }

    /// 列出指定 MC 版本可用的加载器版本（BMCLAPI 优先，官方 meta 备用）
    pub async fn list_loader_versions(
        &self,
        loader_type: &str,
        mc_version: &str,
    ) -> AppResult<Vec<String>> {
        let urls: Vec<String> = match loader_type {
            "vanilla" => return Ok(vec![]),
            "fabric" => vec![
                format!("{BMCLAPI_BASE}/fabric-meta/v2/versions/loader/{mc_version}"),
                format!("https://meta.fabricmc.net/v2/versions/loader/{mc_version}"),
            ],
            "quilt" => vec![
                format!("{BMCLAPI_BASE}/quilt-meta/v3/versions/loader/{mc_version}"),
                format!("https://meta.quiltmc.org/v3/versions/loader/{mc_version}"),
            ],
            "forge" => vec![format!("{BMCLAPI_BASE}/forge/minecraft/{mc_version}")],
            "neoforge" => vec![format!("{BMCLAPI_BASE}/neoforge/list/{mc_version}")],
            "optifine" => vec![format!("{BMCLAPI_BASE}/optifine/{mc_version}")],
            other => return Err(AppError::msg(format!("不支持的加载器类型：{other}"))),
        };

        let mut last_err: Option<AppError> = None;
        for url in urls {
            match self.http.get(&url).send().await {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(json) => {
                        let mut list = extract_loader_versions(&json);
                        // Forge / NeoForge 的 build 列表顺序不可靠（BMCLAPI 1.7.10
                        // 为乱序，首项甚至是最老的构建），统一按版本号降序，
                        // 保证前端默认选中最新构建
                        if loader_type == "forge" || loader_type == "neoforge" {
                            list.sort_by(|a, b| compare_version_desc(a, b));
                        }
                        return Ok(list);
                    }
                    Err(e) => last_err = Some(e.into()),
                },
                Err(e) => last_err = Some(e.into()),
            }
        }
        Err(last_err.unwrap_or_else(|| AppError::msg("获取加载器版本列表失败")))
    }
}

/// 按数字段降序比较版本号（"10.13.4.1614" > "10.13.0.1151"；缺省段视为 0）
fn compare_version_desc(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c| c == '.' || c == '-')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let (x, y) = (
            va.get(i).copied().unwrap_or(0),
            vb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return y.cmp(&x);
        }
    }
    std::cmp::Ordering::Equal
}

/// 从各家 meta 响应中宽松提取版本号列表
/// 兼容：{version} / {loader:{version}} / {type,patch}（OptiFine）/ 纯字符串
fn extract_loader_versions(json: &Value) -> Vec<String> {
    let Some(arr) = json.as_array() else {
        return vec![];
    };
    arr.iter()
        .filter_map(|item| {
            if let Some(v) = item.get("version").and_then(|v| v.as_str()) {
                return Some(v.to_string());
            }
            if let Some(v) = item.pointer("/loader/version").and_then(|v| v.as_str()) {
                return Some(v.to_string());
            }
            if let (Some(t), Some(p)) = (
                item.get("type").and_then(|v| v.as_str()),
                item.get("patch").and_then(|v| v.as_str()),
            ) {
                return Some(format!("{t}-{p}"));
            }
            item.as_str().map(String::from)
        })
        .collect()
}
