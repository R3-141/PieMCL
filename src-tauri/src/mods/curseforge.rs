// CurseForge API v1 封装（需要用户在设置中填入 API key）
//
// 文档：https://docs.curseforge.com/
// - search：关键词 + MC 版本 + modLoaderType 搜索
// - files：某项目的文件列表（按 MC 版本过滤）
// - get_file：单个文件详情（下载地址 / 哈希 / 依赖）
// - resolve_latest：某 modId 在当前环境下的最新兼容文件（依赖解析用）
//
// 限制说明：
// - 作者禁用第三方下载时 downloadUrl 为 null → 明确报错引导官网
// - 依赖只有 modId（无版本引用），解析策略 = 最新兼容文件（relationType 3 = required）
// - 文件哈希：algo 1 = SHA-1

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::core::error::{AppError, AppResult};

const API: &str = "https://api.curseforge.com/v1";
/// MCIMirror 的 CurseForge API 镜像（替换表：api.curseforge.com → mod.mcimirror.top/curseforge），
/// x-api-key 头原样透传；请求失败自动回退官方源
const API_MIRROR: &str = "https://mod.mcimirror.top/curseforge/v1";

/// 实例 loader_type → CurseForge modLoaderType 枚举
pub(crate) fn loader_id(loader: &str) -> Option<i64> {
    match loader {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

fn client(key: &str) -> AppResult<reqwest::Client> {
    if key.trim().is_empty() {
        return Err(AppError::msg(
            "尚未配置 CurseForge API Key，请在「设置 → 模组管理」中填入（Modrinth 无需 key）",
        ));
    }
    crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent(concat!("PieMCL/", env!("CARGO_PKG_VERSION")))
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                "x-api-key",
                reqwest::header::HeaderValue::from_str(key.trim())
                    .map_err(|e| AppError::msg(format!("API Key 非法：{e}")))?,
            );
            h.insert("Accept", reqwest::header::HeaderValue::from_static("application/json"));
            h
        })
        .connect_timeout(std::time::Duration::from_secs(10)))
        .build()
        .map_err(|e| AppError::msg(format!("构建 HTTP 客户端失败：{e}")))
}

async fn get_json(client: &reqwest::Client, url: &str) -> AppResult<serde_json::Value> {
    client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| {
            let hint = if e.status() == Some(reqwest::StatusCode::FORBIDDEN)
                || e.status() == Some(reqwest::StatusCode::UNAUTHORIZED)
            {
                "（API Key 无效或未授权）"
            } else {
                ""
            };
            AppError::msg(format!("CurseForge 请求失败{hint}：{e}"))
        })?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| AppError::msg(format!("CurseForge 响应解析失败：{e}")))
}

/// API GET 请求：MCIMirror 镜像优先，官方源兜底。path_and_query 为相对 API 根的路径（含查询串）
async fn get_json_api(
    client: &reqwest::Client,
    path_and_query: &str,
) -> AppResult<serde_json::Value> {
    let mut last_err: Option<AppError> = None;
    for base in [API_MIRROR, API] {
        match get_json(client, &format!("{base}{path_and_query}")).await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::msg("CurseForge 请求失败：无可用下载源")))
}

// ---------------------------------------------------------------------------
// 数据模型（对外统一为 modrinth.rs 的 SearchHit / OnlineVersion 形状，多带 origin）
// ---------------------------------------------------------------------------

/// CF 文件（对外映射为 OnlineVersion）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: i64,
    pub mod_id: i64,
    /// 展示名（displayName）
    #[serde(alias = "display_name")]
    pub display_name: String,
    pub file_name: String,
    #[serde(default)]
    pub download_url: Option<String>,
    pub file_size: u64,
    /// {value, algo}（algo 1 = sha1）
    #[serde(default)]
    pub hashes: Option<CfHash>,
    #[serde(default, alias = "game_versions")]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<CfDep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfHash {
    pub value: String,
    pub algo: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CfDep {
    pub mod_id: i64,
    /// 1 embedded / 2 optional / 3 required / 4 tool / 5 incompatible / 6 include
    pub relation_type: i64,
}

impl CfFile {
    pub fn sha1(&self) -> Option<String> {
        self.hashes
            .as_ref()
            .filter(|h| h.algo == 1)
            .map(|h| h.value.to_ascii_lowercase())
    }

    /// 是否兼容指定 MC 版本
    pub fn compatible_with(&self, mc_version: &str) -> bool {
        self.game_versions.iter().any(|v| v == mc_version)
    }
}

/// CF 的必需依赖 modId 列表
pub fn required_dep_ids(file: &CfFile) -> Vec<i64> {
    file.dependencies
        .iter()
        .filter(|d| d.relation_type == 3)
        .map(|d| d.mod_id)
        .collect()
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

/// 搜索（返回 (结果, 总数)；统一映射为 SearchHit 形状）
pub async fn search(
    key: &str,
    query: &str,
    mc_version: &str,
    loader: &str,
    sort: &str,
    offset: u32,
) -> AppResult<(Vec<serde_json::Value>, u64)> {
    let client = client(key)?;
    let sort_field = match sort {
        "downloads" => 6,   // TotalDownloads
        "updated" => 3,     // LastUpdated
        "newest" => 3,      // CF 无 Newest，用 LastUpdated
        _ => 2,             // Popularity（相关度场景）
    };
    let index = offset / 20;
    let mut path = format!(
        "/mods/search?gameId=432&searchFilter={q}&sortField={sort_field}&sortOrder=desc&pageSize=20&index={index}",
        q = urlencode(query),
    );
    if !mc_version.is_empty() {
        path.push_str(&format!("&gameVersion={}", urlencode(mc_version)));
    }
    if let Some(lid) = loader_id(loader) {
        path.push_str(&format!("&modLoaderType={lid}"));
    }
    let v = get_json_api(&client, &path).await?;
    let hits = v
        .pointer("/data")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let total = v.pointer("/pagination/totalCount").and_then(|x| x.as_u64()).unwrap_or(0);
    Ok((hits, total))
}

/// 某项目（modId）的文件列表（按 MC 版本 + 加载器过滤）
pub async fn files(
    key: &str,
    mod_id: i64,
    mc_version: &str,
    loader: &str,
) -> AppResult<Vec<CfFile>> {
    let client = client(key)?;
    let mut path = format!(
        "/mods/{mod_id}/files?pageSize=50&gameVersion={}",
        urlencode(mc_version)
    );
    if let Some(lid) = loader_id(loader) {
        path.push_str(&format!("&modLoaderType={lid}"));
    }
    let v = get_json_api(&client, &path).await?;
    let arr = v.pointer("/data").and_then(|x| x.as_array()).cloned().unwrap_or_default();
    Ok(arr
        .iter()
        .filter_map(|f| serde_json::from_value::<CfFile>(f.clone()).ok())
        .collect())
}

/// 单个文件详情
pub async fn get_file(key: &str, mod_id: i64, file_id: i64) -> AppResult<CfFile> {
    let client = client(key)?;
    let v = get_json_api(&client, &format!("/mods/{mod_id}/files/{file_id}")).await?;
    let data = v
        .get("data")
        .cloned()
        .ok_or_else(|| AppError::msg("CurseForge 响应缺少 data 字段"))?;
    serde_json::from_value(data).map_err(|e| AppError::msg(format!("文件数据解析失败：{e}")))
}

/// 项目信息（modId → (slug, name, summary, downloads, icon, author)）
/// 统一映射为 SearchHit 形状（project_id = modId 字符串）
pub async fn get_mod_info(key: &str, mod_ids: &[i64]) -> AppResult<Vec<serde_json::Value>> {
    if mod_ids.is_empty() {
        return Ok(Vec::new());
    }
    let client = client(key)?;
    let body = json!({ "modIds": mod_ids });
    let mut last_err: Option<AppError> = None;
    let mut resp: Option<serde_json::Value> = None;
    for base in [API_MIRROR, API] {
        match client
            .post(format!("{base}/mods"))
            .json(&body)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| AppError::msg(format!("CurseForge 请求失败：{e}")))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| AppError::msg(format!("CurseForge 响应解析失败：{e}")))
        {
            Ok(v) => {
                resp = Some(v);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let resp = resp.ok_or_else(|| {
        last_err.unwrap_or_else(|| AppError::msg("CurseForge 请求失败：无可用下载源"))
    })?;
    Ok(resp
        .pointer("/data")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default())
}

/// 某 modId 在当前环境下的最新兼容文件（依赖解析 / 更新检查）
pub async fn resolve_latest(
    key: &str,
    mod_id: i64,
    mc_version: &str,
    loader: &str,
) -> AppResult<Option<CfFile>> {
    let list = files(key, mod_id, mc_version, loader).await?;
    Ok(list.into_iter().next())
}

/// 简易 URL 编码
pub(crate) fn urlencode(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_mapping() {
        assert_eq!(loader_id("forge"), Some(1));
        assert_eq!(loader_id("fabric"), Some(4));
        assert_eq!(loader_id("neoforge"), Some(6));
        assert_eq!(loader_id("vanilla"), None);
    }

    #[test]
    fn sha1_extraction() {
        let f = CfFile {
            id: 1,
            mod_id: 2,
            display_name: "X 1.0".into(),
            file_name: "x-1.0.jar".into(),
            download_url: None,
            file_size: 10,
            hashes: Some(CfHash { value: "AB".into(), algo: 1 }),
            game_versions: vec!["1.21.4".into()],
            dependencies: vec![CfDep { mod_id: 9, relation_type: 3 }],
        };
        assert_eq!(f.sha1().as_deref(), Some("ab"));
        assert_eq!(required_dep_ids(&f), vec![9i64]);
        // md5 哈希不当作 sha1
        let f2 = CfFile { hashes: Some(CfHash { value: "x".into(), algo: 2 }), ..f };
        assert!(f2.sha1().is_none());
    }

    #[test]
    fn url_encoding() {
        assert_eq!(urlencode("sodium+extra/fabric"), "sodium%2Bextra%2Ffabric");
    }
}
