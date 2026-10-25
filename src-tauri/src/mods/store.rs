// 资源安装来源记录：{版本文件夹}/.pie/mods.json
//
// 记录每个社区资源文件（模组 / 数据包 / 光影 / 资源包）的来源
// （Modrinth / CurseForge / 本地），供：
// - 更新检查（对比线上最新版本）
// - 整合包导出（生成 .mrpack 的 files 清单，仅模组）
// key 为 "{类型前缀}:{基文件名}"（如 "mods:x.jar"、"resource_pack:y.zip"；
// 基名去掉 .disabled 后缀，启用状态变化不影响键）。
// 旧版本数据无前缀（Windows 文件名不含冒号），加载时自动补 "mods:" 前缀。
// 文件缺失或损坏时静默回落空记录（来源信息丢失不应阻塞管理功能）。

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ResourceType;

/// 单个文件的来源信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSource {
    /// modrinth / curseforge / local
    pub origin: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    /// 平台上的版本号字符串（展示用，区别于文件内元数据版本）
    #[serde(default)]
    pub version_number: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
}

/// 来源记录表
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModSources(pub HashMap<String, ModSource>);

impl ModSources {
    /// 类型化 key："{prefix}:{基名}"
    pub fn key(kind: ResourceType, base_name: &str) -> String {
        format!("{}:{}", kind.prefix(), base_name)
    }

    /// mods.json 路径：{版本文件夹}/.pie/mods.json
    fn path(instance_dir: &Path) -> std::path::PathBuf {
        instance_dir.join(".pie").join("mods.json")
    }

    pub fn load(instance_dir: &Path) -> Self {
        let path = Self::path(instance_dir);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut map: HashMap<String, ModSource> =
                    serde_json::from_str(&text).unwrap_or_default();
                // 兼容旧数据：无前缀 key（文件名不含冒号）一律视为模组记录
                let legacy: Vec<String> =
                    map.keys().filter(|k| !k.contains(':')).cloned().collect();
                for k in legacy {
                    if let Some(v) = map.remove(&k) {
                        map.insert(Self::key(ResourceType::Mod, &k), v);
                    }
                }
                Self(map)
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, instance_dir: &Path) -> Result<(), String> {
        let path = Self::path(instance_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建实例元数据目录失败：{e}"))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化资源来源失败：{e}"))?;
        std::fs::write(&path, text).map_err(|e| format!("保存资源来源失败：{e}"))
    }

    /// 读取某类型某基名的来源记录
    pub fn get(&self, kind: ResourceType, base_name: &str) -> Option<&ModSource> {
        self.0.get(&Self::key(kind, base_name))
    }

    /// 写入某类型某基名的来源记录
    pub fn insert(&mut self, kind: ResourceType, base_name: &str, source: ModSource) {
        self.0
            .insert(Self::key(kind, base_name), source);
    }

    /// 删除某类型某基名的来源记录
    pub fn remove(&mut self, kind: ResourceType, base_name: &str) {
        self.0.remove(&Self::key(kind, base_name));
    }

    /// 遍历某类型的记录（返回基名，已剥前缀）
    pub fn iter_kind<'a>(
        &'a self,
        kind: ResourceType,
    ) -> impl Iterator<Item = (&'a str, &'a ModSource)> + 'a {
        let filter_prefix = format!("{}:", kind.prefix());
        let strip_len = filter_prefix.len();
        self.0
            .iter()
            .filter(move |(k, _)| k.starts_with(&filter_prefix))
            .map(move |(k, v)| (&k[strip_len..], v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_keys_get_mods_prefix() {
        let dir = std::env::temp_dir().join(format!("piemcl-store-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".pie")).unwrap();
        std::fs::write(
            dir.join(".pie").join("mods.json"),
            r#"{"a.jar":{"origin":"modrinth","projectId":"p1"}}"#,
        )
        .unwrap();

        let sources = ModSources::load(&dir);
        assert!(sources.get(ResourceType::Mod, "a.jar").is_some());
        assert!(sources.get(ResourceType::Datapack, "a.jar").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn typed_insert_and_iter() {
        let mut s = ModSources::default();
        s.insert(
            ResourceType::Shader,
            "fancy.zip",
            ModSource {
                origin: "modrinth".into(),
                project_id: Some("p2".into()),
                version_id: None,
                version_number: None,
                sha1: None,
                download_url: None,
            },
        );
        assert!(s.get(ResourceType::Shader, "fancy.zip").is_some());
        assert!(s.get(ResourceType::Mod, "fancy.zip").is_none());
        let kvs: Vec<_> = s.iter_kind(ResourceType::Shader).collect();
        assert_eq!(kvs.len(), 1);
        assert_eq!(kvs[0].0, "fancy.zip");
        s.remove(ResourceType::Shader, "fancy.zip");
        assert!(s.get(ResourceType::Shader, "fancy.zip").is_none());
    }
}
