// 版本清单与版本 JSON 的数据模型
//
// version.json 本体结构复杂且随版本演进，这里只对清单做强类型建模；
// 单个版本 JSON 以 serde_json::Value 承载，合并逻辑见 resolve.rs。

use serde::{Deserialize, Serialize};

/// Mojang version_manifest_v2 顶层结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

/// 清单中的单个版本条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub time: String,
    pub release_time: String,
}

/// 发给前端的轻量版本条目（list_mc_versions 返回）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McVersionEntry {
    pub id: String,
    pub version_type: String,
    pub release_time: String,
}

impl From<&ManifestVersion> for McVersionEntry {
    fn from(v: &ManifestVersion) -> Self {
        Self {
            id: v.id.clone(),
            version_type: v.version_type.clone(),
            release_time: v.release_time.clone(),
        }
    }
}

/// 清单缓存 sidecar 元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestMeta {
    /// 获取时间（epoch 秒）
    pub fetched_at: i64,
    /// 实际命中的来源（镜像 / 官方）
    pub source: String,
}
