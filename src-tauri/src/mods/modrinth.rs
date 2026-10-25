// Modrinth API v2 封装（无需用户 key）
//
// 文档：https://docs.modrinth.com/api-spec/
// - search：关键词 + MC 版本 + 加载器 facet 搜索（分页）
// - list_versions：某项目的兼容版本列表（过滤失败自动回退全量）
// - get_version / get_versions_batch：版本详情（primary 文件 + 依赖）
// - resolve_deps：递归收集必需依赖（≤3 层），标注是否已安装
// 所有请求经 reqwest（Rust 侧），User-Agent 标识 PieMCL（Modrinth 要求）。
// 下载 URL（cdn.modrinth.com）由调用方交给下载引擎（sha1 校验）。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::error::{AppError, AppResult};

const API: &str = "https://api.modrinth.com/v2";
/// MCIMirror 的 Modrinth API 镜像（替换表：api.modrinth.com → mod.mcimirror.top/modrinth），
/// 路径与参数同上游一致，仅换域名；官方源失败时兜底
const API_MIRROR: &str = "https://mod.mcimirror.top/modrinth/v2";
const USER_AGENT: &str = concat!("PieMCL/", env!("CARGO_PKG_VERSION"));

fn client() -> AppResult<reqwest::Client> {
    crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15)))
        .build()
        .map_err(|e| AppError::msg(format!("构建 HTTP 客户端失败：{e}")))
}

async fn get_json(client: &reqwest::Client, url: &str) -> AppResult<serde_json::Value> {
    client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| AppError::msg(format!("Modrinth 请求失败：{e}")))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| AppError::msg(format!("Modrinth 响应解析失败：{e}")))
}

/// API 请求：官方源优先（热门列表等元信息对延迟敏感），MCIMirror 兜底。
/// path_and_query 为相对 API 根的路径（含查询串）。
async fn get_json_api(
    client: &reqwest::Client,
    path_and_query: &str,
) -> AppResult<serde_json::Value> {
    let mut last_err: Option<AppError> = None;
    for base in [API, API_MIRROR] {
        match get_json(client, &format!("{base}{path_and_query}")).await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err
        .unwrap_or_else(|| AppError::msg("Modrinth 请求失败：无可用下载源")))
}

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 搜索结果条目
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
    pub author: Option<String>,
    /// 来源平台：modrinth / curseforge
    pub origin: String,
    /// 展示分类（Modrinth display_categories，含加载器与内容分类）
    #[serde(default)]
    pub display_categories: Vec<String>,
    /// 最近更新时间（ISO 8601）
    #[serde(default)]
    pub date_modified: Option<String>,
}

/// 版本内的单个可下载文件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    #[serde(default)]
    pub size: u64,
    /// sha1 小写 hex（缺失时不校验）
    #[serde(default)]
    pub sha1: Option<String>,
}

/// 版本的依赖引用（Modrinth 返回 snake_case，前端需要 camelCase → alias 双向兼容）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepRef {
    #[serde(default, alias = "version_id")]
    pub version_id: Option<String>,
    #[serde(default, alias = "project_id")]
    pub project_id: Option<String>,
    /// required / optional / incompatible
    #[serde(default, alias = "dependency_type")]
    pub dependency_type: String,
}

/// 单个版本详情
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineVersion {
    pub id: String,
    #[serde(alias = "project_id")]
    pub project_id: String,
    #[serde(alias = "version_number")]
    pub version_number: String,
    #[serde(default, alias = "version_type")]
    pub version_type: String,
    #[serde(default, alias = "game_versions")]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<DepRef>,
    pub files: Vec<VersionFile>,
}

impl OnlineVersion {
    /// 主文件（primary 优先，无则取第一个）
    pub fn primary_file(&self) -> Option<&VersionFile> {
        self.files.iter().find(|f| f.primary).or_else(|| self.files.first())
    }

    /// 是否兼容指定 MC 版本（不校验加载器；数据包 / 光影 / 资源包用）
    pub fn compatible_mc(&self, mc_version: &str) -> bool {
        self.game_versions.iter().any(|v| v == mc_version)
    }

    /// 是否兼容指定 MC 版本与加载器
    pub fn compatible_with(&self, mc_version: &str, loader: &str) -> bool {
        self.compatible_mc(mc_version)
            && self.loaders.iter().any(|l| loader_eq(l, loader))
    }

    /// 必需依赖的 version_id 列表
    pub fn required_dep_ids(&self) -> Vec<String> {
        self.dependencies
            .iter()
            .filter(|d| d.dependency_type == "required")
            .filter_map(|d| d.version_id.clone())
            .collect()
    }
}

/// Modrinth 加载器名与实例 loader_type 对齐（quilt 可装 fabric 模组的场景暂不放宽）
pub(crate) fn loader_eq(modrinth_loader: &str, instance_loader: &str) -> bool {
    modrinth_loader.eq_ignore_ascii_case(instance_loader)
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

/// 搜索（返回 (结果, 总数)）。project_type：mod / datapack / shader / resource_pack。
/// 非 mod 类型不按加载器过滤（其 loaders 取值与实例加载器无对应关系）。
pub async fn search(
    query: &str,
    mc_version: &str,
    loader: Option<&str>,
    category: Option<&str>,
    sort: &str, // relevance / downloads / newest / updated
    offset: u32,
    project_type: &str,
) -> AppResult<(Vec<SearchHit>, u64)> {
    let mut facets = vec![json!([format!("project_type:{project_type}")])];
    if !mc_version.is_empty() {
        facets.push(json!([format!("versions:{mc_version}")]));
    }
    if let Some(l) = loader {
        if !l.is_empty() && l != "vanilla" {
            facets.push(json!([format!("categories:{l}")]));
        }
    }
    if let Some(c) = category {
        if !c.is_empty() {
            facets.push(json!([format!("categories:{c}")]));
        }
    }
    let index = match sort {
        "downloads" => "downloads",
        "newest" => "newest",
        "updated" => "updated",
        _ => "relevance",
    };
    let encoded_query = urlencoding_encode(query);
    let encoded_facets = urlencoding_encode(&json!(facets).to_string());
    let path = format!(
        "/search?limit=20&offset={offset}&index={index}&query={encoded_query}&facets={encoded_facets}"
    );
    let client = client()?;
    let v = get_json_api(&client, &path).await?;
    let hits: Vec<SearchHit> = v
        .get("hits")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|h| {
                    Some(SearchHit {
                        project_id: h.get("project_id")?.as_str()?.to_string(),
                        slug: h
                            .get("slug")
                            .and_then(|x| x.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        title: h
                            .get("title")
                            .and_then(|x| x.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        description: h
                            .get("description")
                            .and_then(|x| x.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        downloads: h.get("downloads").and_then(|x| x.as_u64()).unwrap_or(0),
                        // 空字符串视为无图标
                        icon_url: h
                            .get("icon_url")
                            .and_then(|x| x.as_str())
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                        author: h
                            .get("author")
                            .and_then(|x| x.as_str())
                            .map(String::from),
                        origin: "modrinth".into(),
                        display_categories: h
                            .get("display_categories")
                            .and_then(|x| x.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|c| c.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        date_modified: h
                            .get("date_modified")
                            .and_then(|x| x.as_str())
                            .map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let total = v.get("total_hits").and_then(|x| x.as_u64()).unwrap_or(0);
    Ok((hits, total))
}

/// 某项目的版本列表：优先按 MC 版本（模组额外按加载器）过滤，过滤为空时回退全量
pub async fn list_versions(
    project_id: &str,
    mc_version: &str,
    loader: &str,
    project_type: &str,
) -> AppResult<Vec<OnlineVersion>> {
    let client = client()?;
    let game_versions_param = format!(
        "game_versions={}",
        urlencoding_encode(&json!([mc_version]).to_string()),
    );
    // 非 mod 类型的 loaders 取值（datapack / minecraft / iris 等）与实例加载器
    // 无对应关系，只按 MC 版本过滤
    let filtered_path = if project_type == "mod" {
        format!(
            "/project/{project_id}/version?loaders={loaders}&{game_versions_param}",
            loaders = urlencoding_encode(&json!([loader]).to_string()),
        )
    } else {
        format!("/project/{project_id}/version?{game_versions_param}")
    };
    let v = get_json_api(&client, &filtered_path).await?;
    let mut versions = parse_versions(&v);
    if versions.is_empty() {
        // 无完全兼容版本：回退全量，由前端按 compatible 标注展示
        let v = get_json_api(&client, &format!("/project/{project_id}/version")).await?;
        versions = parse_versions(&v);
    }
    Ok(versions)
}

/// 单个版本详情
pub async fn get_version(version_id: &str) -> AppResult<OnlineVersion> {
    let client = client()?;
    let v = get_json_api(&client, &format!("/version/{version_id}")).await?;
    serde_json::from_value(v).map_err(|e| AppError::msg(format!("版本数据解析失败：{e}")))
}

/// 批量获取版本
pub async fn get_versions_batch(version_ids: &[String]) -> AppResult<Vec<OnlineVersion>> {
    if version_ids.is_empty() {
        return Ok(Vec::new());
    }
    let client = client()?;
    let v = get_json_api(
        &client,
        &format!("/versions?ids={}", urlencoding_encode(&json!(version_ids).to_string())),
    )
    .await?;
    Ok(parse_versions(&v).find_or_first_ok(version_ids))
}

/// 批量获取项目信息（slug / title），返回 project_id → (slug, title)
pub async fn get_projects_batch(project_ids: &[String]) -> AppResult<Vec<(String, String, String)>> {
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }
    let client = client()?;
    let v = get_json_api(
        &client,
        &format!("/projects?ids={}", urlencoding_encode(&json!(project_ids).to_string())),
    )
    .await?;
    Ok(v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some((
                        p.get("id")?.as_str()?.to_string(),
                        p.get("slug")?.as_str()?.to_string(),
                        p.get("title")?.as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
}

/// 某项目在指定环境下的最新版本（更新检查用）。
/// 模组按 MC 版本 + 加载器匹配；其余类型只按 MC 版本。
pub async fn latest_compatible(
    project_id: &str,
    mc_version: &str,
    loader: &str,
    project_type: &str,
) -> AppResult<Option<OnlineVersion>> {
    let versions = list_versions(project_id, mc_version, loader, project_type).await?;
    // list_versions 已按日期倒序（Modrinth 默认），取第一个完全兼容者
    let matched = if project_type == "mod" {
        versions.into_iter().find(|v| v.compatible_with(mc_version, loader))
    } else {
        versions.into_iter().find(|v| v.compatible_mc(mc_version))
    };
    Ok(matched)
}

// ---------------------------------------------------------------------------
// 依赖解析（两段式安装的第一段）
// ---------------------------------------------------------------------------

/// 计划安装的单个依赖
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDep {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub version_id: String,
    pub version_number: String,
    pub file: VersionFile,
    /// 该前置是否已安装（按 project_id / slug 匹配现有模组）
    pub already_installed: bool,
    /// 来源平台：modrinth / curseforge
    pub origin: String,
}

/// 安装计划：主文件 + 依赖树（两段式确认）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub main: PlanDep,
    /// 缺失的必需前置（已安装的不重复列出）
    pub deps: Vec<PlanDep>,
    /// 可选依赖（作者标记 optional 的"推荐安装"项，自动定位最新兼容版本）
    pub recommended: Vec<PlanDep>,
    /// 仅给出 project_id 而无法定位版本的依赖（提示用户手动处理）
    pub unknown_deps: Vec<String>,
}

/// 递归解析必需依赖（BFS，深度 ≤3），过滤已安装项。
/// installed_keys：已安装模组的标识集合（project_id / slug / mod_id 任一命中即视为已装）。
pub async fn resolve_install_plan(
    version: &OnlineVersion,
    mc_version: &str,
    loader: &str,
    installed_keys: &HashSet<String>,
) -> AppResult<InstallPlan> {
    let file = version.primary_file().ok_or_else(|| {
        AppError::msg("该版本没有可下载的文件（作者可能未上传文件）")
    })?;
    let (main_slug, main_title) = project_slug_title(&version.project_id).await?;
    let main = PlanDep {
        project_id: version.project_id.clone(),
        slug: main_slug,
        title: main_title,
        version_id: version.id.clone(),
        version_number: version.version_number.clone(),
        file: file.clone(),
        already_installed: false,
        origin: "modrinth".into(),
    };

    // 依赖分组：
    // - required 且带 version_id → 直接 BFS 展开
    // - required 仅 project_id（最常见的声明方式）→ 自动定位最新兼容版本后同样展开
    // - optional → 汇总为「推荐安装」（不递归其依赖，避免列表爆炸）
    let mut required_pid_only: Vec<String> = Vec::new();
    let mut optional_pids: Vec<String> = vec![];
    for d in &version.dependencies {
        if d.version_id.is_some() {
            continue; // version_id 型由下方 BFS 统一处理
        }
        let Some(pid) = &d.project_id else { continue };
        match d.dependency_type.as_str() {
            "required" => required_pid_only.push(pid.clone()),
            "optional" => optional_pids.push(pid.clone()),
            _ => {}
        }
    }

    // BFS 逐层收集必需依赖
    let mut collected: Vec<(OnlineVersion, String, String)> = Vec::new(); // (version, slug, title)
    let mut collected_pids: HashSet<String> = HashSet::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut unknown_deps: Vec<String> = Vec::new();
    let mut queue: Vec<String> = version.required_dep_ids();
    let mut pid_queue: Vec<String> = required_pid_only;
    let mut depth = 0;
    loop {
        if (queue.is_empty() && pid_queue.is_empty()) || depth >= 4 {
            break;
        }
        // ---- 批 1：version_id 型必需依赖 ----
        let batch: Vec<String> = queue
            .drain(..)
            .filter(|id| seen.insert(id.clone()))
            .collect();
        if !batch.is_empty() {
            let versions = get_versions_batch(&batch).await?;
            let mut next: Vec<String> = Vec::new();
            for dep_v in &versions {
                if installed_keys.contains(&dep_v.project_id) {
                    continue;
                }
                next.extend(dep_v.required_dep_ids());
                // 仅 project_id 声明的依赖转入定位队列
                for d in &dep_v.dependencies {
                    if d.dependency_type == "required"
                        && d.version_id.is_none()
                        && let Some(pid) = &d.project_id
                    {
                        pid_queue.push(pid.clone());
                    }
                    if d.dependency_type == "optional"
                        && let Some(pid) = &d.project_id
                    {
                        optional_pids.push(pid.clone());
                    }
                }
            }
            let project_ids: Vec<String> = versions.iter().map(|v| v.project_id.clone()).collect();
            let projects = get_projects_batch(&project_ids).await?;
            for dep_v in versions {
                if installed_keys.contains(&dep_v.project_id) {
                    continue; // 已安装：跳过（不再展示，也不再展开）
                }
                let (slug, title) = projects
                    .iter()
                    .find(|(pid, _, _)| pid == &dep_v.project_id)
                    .map(|(_, s, t)| (s.clone(), t.clone()))
                    .unwrap_or_default();
                collected_pids.insert(dep_v.project_id.clone());
                collected.push((dep_v, slug, title));
            }
            queue = next;
        }

        // ---- 批 2：仅 project_id 型必需依赖（自动定位最新兼容版本）----
        let batch: Vec<String> = pid_queue
            .drain(..)
            .filter(|pid| seen.insert(pid.clone()))
            .collect();
        if !batch.is_empty() {
            let projects = get_projects_batch(&batch).await?;
            for pid in batch {
                let (slug, title) = projects
                    .iter()
                    .find(|(p, _, _)| p == &pid)
                    .map(|(_, s, t)| (s.clone(), t.clone()))
                    .unwrap_or_default();
                let installed_hit = installed_keys.contains(&pid) || installed_keys.contains(&slug);
                if installed_hit {
                    continue;
                }
                match latest_compatible(&pid, mc_version, loader, "mod").await {
                    Ok(Some(dep_v)) => {
                        // 展开该版本自身的依赖（可能再产生两种队列项）
                        for d in &dep_v.dependencies {
                            if d.dependency_type == "required" {
                                if let Some(vid) = &d.version_id {
                                    queue.push(vid.clone());
                                } else if let Some(dpid) = &d.project_id {
                                    pid_queue.push(dpid.clone());
                                }
                            } else if d.dependency_type == "optional"
                                && let Some(dpid) = &d.project_id
                            {
                                optional_pids.push(dpid.clone());
                            }
                        }
                        if let Some(f) = dep_v.primary_file() {
                            collected_pids.insert(dep_v.project_id.clone());
                            collected.push((dep_v, slug, title));
                        }
                    }
                    Ok(None) => {
                        unknown_deps.push(if title.is_empty() {
                            format!("Modrinth 项目 {pid}")
                        } else {
                            title.clone()
                        });
                    }
                    Err(_) => {
                        unknown_deps.push(if title.is_empty() {
                            format!("Modrinth 项目 {pid}")
                        } else {
                            title.clone()
                        });
                    }
                }
            }
        }
        depth += 1;
    }

    let deps = collected
        .into_iter()
        .filter_map(|(v, slug, title)| {
            let file = v.primary_file()?;
            Some(PlanDep {
                project_id: v.project_id.clone(),
                slug,
                title,
                version_id: v.id.clone(),
                version_number: v.version_number.clone(),
                file: file.clone(),
                already_installed: false,
                origin: "modrinth".into(),
            })
        })
        // 自身已在主文件中（循环依赖防护）
        .filter(|d| d.version_id != version.id)
        .collect();

    // ---- 推荐安装（optional）：主件与已收集依赖声明的可选依赖，自动定位最新
    // 兼容版本；已装 / 已在 deps 中的不再出现；定位失败静默跳过 ----
    let mut recommended: Vec<PlanDep> = Vec::new();
    let mut rec_seen: HashSet<String> = HashSet::new();
    for pid in optional_pids {
        if !rec_seen.insert(pid.clone()) {
            continue;
        }
        if installed_keys.contains(&pid)
            || collected_pids.contains(&pid)
            || pid == version.project_id
        {
            continue;
        }
        let Ok(Some(rec_v)) = latest_compatible(&pid, mc_version, loader, "mod").await else {
            continue;
        };
        if installed_keys.contains(&rec_v.project_id) {
            continue;
        }
        if let Some(f) = rec_v.primary_file() {
            let (slug, title) = project_slug_title(&rec_v.project_id).await.unwrap_or_default();
            recommended.push(PlanDep {
                project_id: rec_v.project_id.clone(),
                slug,
                title,
                version_id: rec_v.id.clone(),
                version_number: rec_v.version_number.clone(),
                file: f.clone(),
                already_installed: false,
                origin: "modrinth".into(),
            });
        }
    }

    Ok(InstallPlan {
        main,
        deps,
        recommended,
        unknown_deps,
    })
}

/// 查询项目 slug 与 title
async fn project_slug_title(project_id: &str) -> AppResult<(String, String)> {
    let client = client()?;
    let v = get_json_api(&client, &format!("/project/{project_id}")).await?;
    Ok((
        v.get("slug").and_then(|x| x.as_str()).unwrap_or_default().into(),
        v.get("title").and_then(|x| x.as_str()).unwrap_or_default().into(),
    ))
}

/// 解析版本数组响应
fn parse_versions(v: &serde_json::Value) -> Vec<OnlineVersion> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|x| serde_json::from_value(x.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// 简易 URL 编码（reqwest 不自动编码 query 里的 JSON 结构）
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

trait FindOrFirstOk {
    /// 按传入顺序整理（请求结果顺序不保证）
    fn find_or_first_ok(self, order: &[String]) -> Vec<OnlineVersion>;
}

impl FindOrFirstOk for Vec<OnlineVersion> {
    fn find_or_first_ok(self, order: &[String]) -> Vec<OnlineVersion> {
        let mut out: Vec<OnlineVersion> = Vec::with_capacity(order.len());
        for id in order {
            if let Some(v) = self.iter().find(|v| &v.id == id) {
                out.push(v.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compat_and_dep_extraction() {
        let v: OnlineVersion = serde_json::from_value(json!({
            "id": "ver1", "project_id": "proj1", "version_number": "1.2.3",
            "version_type": "release",
            "game_versions": ["1.21.4", "1.21.3"],
            "loaders": ["fabric"],
            "dependencies": [
                { "version_id": "depv1", "project_id": "depp1", "dependency_type": "required" },
                { "version_id": null, "project_id": "depp2", "dependency_type": "optional" },
                { "version_id": "depv3", "project_id": "depp3", "dependency_type": "incompatible" }
            ],
            "files": [
                { "url": "https://cdn.example.com/b.jar", "filename": "b.jar", "primary": false, "size": 1, "sha1": null },
                { "url": "https://cdn.example.com/a.jar", "filename": "a.jar", "primary": true, "size": 2, "sha1": "aa" }
            ]
        }))
        .unwrap();
        assert!(v.compatible_with("1.21.4", "fabric"));
        assert!(!v.compatible_with("1.20.1", "fabric"));
        assert!(!v.compatible_with("1.21.4", "forge"));
        assert_eq!(v.primary_file().unwrap().filename, "a.jar");
        assert_eq!(v.required_dep_ids(), vec!["depv1".to_string()]);
    }

    #[test]
    fn loader_eq_matches_case() {
        assert!(loader_eq("Fabric", "fabric"));
        assert!(loader_eq("quilt", "quilt"));
        assert!(!loader_eq("forge", "fabric"));
    }

    #[test]
    fn url_encoding() {
        assert_eq!(urlencoding_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }
}
