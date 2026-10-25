// 模组 jar 元数据解析
//
// 逐级降级解析（任一级成功即返回，全部失败回空结构 → 前端以文件名展示）：
// 1. fabric.mod.json（Fabric / Quilt）
// 2. META-INF/mods.toml（Forge）与 META-INF/neoforge.mods.toml（NeoForge 20.2+）
// 3. META-INF/MANIFEST.MF（旧 FML 模组的 Implementation-Title/Version）
//
// 所有解析失败都不产生错误，只影响该行的展示信息。

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

/// 平台内置依赖键：这些是加载器/游戏自身，不视为可安装的“前置模组”
const SYSTEM_DEPS: &[&str] = &[
    "minecraft",
    "java",
    "fabricloader",
    "fabricloader.net.fabricmc.fabric",
    "javafml",
    "forge",
    "neoforge",
    "fml",
    "quilt_loader",
];

pub(crate) fn is_system_dep(id: &str) -> bool {
    SYSTEM_DEPS.contains(&id) || SYSTEM_DEPS.iter().any(|s| id.starts_with(&format!("{s} ")))
}

/// 从 jar 元数据解析出的模组描述（全部可选，缺失即降级展示）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedMod {
    /// 模组自身 id（fabric id / mods.toml modId / MANIFEST Implementation-Title）
    pub mod_id: Option<String>,
    /// 显示名
    pub name: Option<String>,
    pub version: Option<String>,
    /// 元数据自报加载器：fabric / forge / neoforge；MANIFEST 兜底无法判定为 None
    pub loader: Option<String>,
    /// 必需前置依赖 id（已剔除平台内置键）
    pub depends: Vec<String>,
}

/// 解析单个模组 jar；任何异常都回空结构（不阻塞列表）
pub fn parse_jar(path: &Path) -> ParsedMod {
    let Ok(file) = std::fs::File::open(path) else {
        return ParsedMod::default();
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return ParsedMod::default();
    };

    // 1. fabric.mod.json（Fabric / Quilt）
    if let Some(m) = read_entry_text(&mut archive, "fabric.mod.json")
        .as_deref()
        .and_then(parse_fabric_json)
    {
        return m;
    }
    // 2. NeoForge / Forge 的 mods.toml（NeoForge 20.2+ 文件名带前缀，优先探测）
    for (name, loader) in [
        ("META-INF/neoforge.mods.toml", "neoforge"),
        ("META-INF/mods.toml", "forge"),
    ] {
        if let Some(m) = read_entry_text(&mut archive, name)
            .as_deref()
            .and_then(|t| parse_mods_toml(t, loader))
        {
            return m;
        }
    }
    // 3. MANIFEST.MF
    if let Some(m) = read_entry_text(&mut archive, "META-INF/MANIFEST.MF")
        .as_deref()
        .and_then(parse_manifest)
    {
        return m;
    }
    ParsedMod::default()
}

/// 读取 zip 内单个文本条目
fn read_entry_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    Some(text)
}

/// fabric.mod.json：depends 的值可能是字符串 / 数组 / 对象，只取键名
fn parse_fabric_json(text: &str) -> Option<ParsedMod> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let id = v.get("id").and_then(|x| x.as_str())?.to_string();
    let depends: HashSet<String> = v
        .get("depends")
        .and_then(|x| x.as_object())
        .map(|m| {
            m.keys()
                .filter(|k| !is_system_dep(k))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let name = v
        .get("name")
        .and_then(|x| x.as_str())
        .map(String::from)
        .unwrap_or_else(|| id.clone());
    let version = v.get("version").and_then(|x| x.as_str()).map(String::from);
    Some(ParsedMod {
        mod_id: Some(id),
        name: Some(name),
        version,
        loader: Some("fabric".into()),
        depends: depends.into_iter().collect(),
    })
}

/// Forge/NeoForge mods.toml：
/// [mods] 数组给出自身；[dependencies.<modId>] 数组的每项按 mandatory / optional 判定必要性
/// （Forge 的 mandatory 默认 false，NeoForge 的 optional 默认 true；两者都缺省视为必需——
/// 保守多提示优于漏报，且用户要求依赖缺失“提示但不阻止”）
fn parse_mods_toml(text: &str, loader: &str) -> Option<ParsedMod> {
    let root: toml::Value = toml::from_str(text).ok()?;
    let first = root
        .get("mods")?
        .as_array()?
        .first()?
        .as_table()?
        .clone();
    let mod_id = first.get("modId")?.as_str()?.to_string();
    let name = first
        .get("displayName")
        .and_then(|x| x.as_str())
        .map(String::from)
        .unwrap_or_else(|| mod_id.clone());
    let version = first.get("version").and_then(|x| x.as_str()).map(String::from);

    let mut depends: Vec<String> = Vec::new();
    if let Some(groups) = root.get("dependencies").and_then(|x| x.as_table()) {
        for list in groups.values() {
            let Some(arr) = list.as_array() else { continue };
            for dep in arr {
                let required = match (
                    dep.get("mandatory").and_then(|x| x.as_bool()),
                    dep.get("optional").and_then(|x| x.as_bool()),
                ) {
                    (Some(m), _) => m,
                    (None, Some(o)) => !o,
                    (None, None) => true,
                };
                if !required {
                    continue;
                }
                if let Some(id) = dep.get("modId").and_then(|x| x.as_str()) {
                    if !is_system_dep(id) && !depends.iter().any(|d| d == id) {
                        depends.push(id.to_string());
                    }
                }
            }
        }
    }
    Some(ParsedMod {
        mod_id: Some(mod_id),
        name: Some(name),
        version,
        loader: Some(loader.into()),
        depends,
    })
}

/// MANIFEST.MF 的 Implementation-Title / Implementation-Version（旧 FML 模组）
fn parse_manifest(text: &str) -> Option<ParsedMod> {
    let mut title: Option<String> = None;
    let mut version: Option<String> = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Implementation-Title:") {
            let t = v.trim();
            if !t.is_empty() {
                title = Some(t.to_string());
            }
        } else if let Some(v) = line.strip_prefix("Implementation-Version:") {
            let t = v.trim();
            if !t.is_empty() {
                version = Some(t.to_string());
            }
        }
    }
    let title = title?;
    Some(ParsedMod {
        mod_id: Some(title.clone()),
        name: Some(title),
        version,
        loader: None,
        depends: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 在内存中构造模组 jar
    fn build_jar(entries: &[(&str, &str)]) -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(buf);
        let opts: zip::write::SimpleFileOptions = Default::default();
        for (name, content) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(content.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn parses_fabric_mod_json() {
        let bytes = build_jar(&[(
            "fabric.mod.json",
            r#"{
                "id": "sodium-extras", "name": "Sodium Extras", "version": "1.0.2",
                "depends": { "fabricloader": ">=0.15", "minecraft": "~1.21", "fabric-api": "*", "sodium": "0.6.x" },
                "breaks": {}
            }"#,
        )]);
        let dir = std::env::temp_dir();
        let path = dir.join(format!("piemcl-parse-{}.jar", uuid::Uuid::new_v4()));
        std::fs::write(&path, &bytes).unwrap();
        let m = parse_jar(&path);
        std::fs::remove_file(&path).ok();

        assert_eq!(m.mod_id.as_deref(), Some("sodium-extras"));
        assert_eq!(m.name.as_deref(), Some("Sodium Extras"));
        assert_eq!(m.version.as_deref(), Some("1.0.2"));
        assert_eq!(m.loader.as_deref(), Some("fabric"));
        // 平台键被剔除，实际前置保留
        let mut deps = m.depends.clone();
        deps.sort();
        assert_eq!(deps, vec!["fabric-api", "sodium"]);
    }

    #[test]
    fn parses_forge_mods_toml() {
        let bytes = build_jar(&[(
            "META-INF/mods.toml",
            r#"
            modLoader = "javafml"
            loaderVersion = "[47,)"
            [[mods]]
            modId = "create"
            version = "6.0.0"
            displayName = "Create"
            [[dependencies.create]]
            modId = "forge"
            mandatory = true
            [[dependencies.create]]
            modId = "jei"
            mandatory = true
            [[dependencies.create]]
            modId = "jei_extras"
            mandatory = false
        "#,
        )]);
        let dir = std::env::temp_dir();
        let path = dir.join(format!("piemcl-parse-{}.jar", uuid::Uuid::new_v4()));
        std::fs::write(&path, &bytes).unwrap();
        let m = parse_jar(&path);
        std::fs::remove_file(&path).ok();

        assert_eq!(m.mod_id.as_deref(), Some("create"));
        assert_eq!(m.name.as_deref(), Some("Create"));
        assert_eq!(m.loader.as_deref(), Some("forge"));
        assert_eq!(m.depends, vec!["jei"]);
    }

    #[test]
    fn parses_manifest_and_degrades() {
        // MANIFEST 兜底
        let bytes = build_jar(&[(
            "META-INF/MANIFEST.MF",
            "Manifest-Version: 1.0\nImplementation-Title: Old Mod\nImplementation-Version: 3.2.1\n",
        )]);
        let dir = std::env::temp_dir();
        let path = dir.join(format!("piemcl-parse-{}.jar", uuid::Uuid::new_v4()));
        std::fs::write(&path, &bytes).unwrap();
        let m = parse_jar(&path);
        std::fs::remove_file(&path).ok();
        assert_eq!(m.mod_id.as_deref(), Some("Old Mod"));
        assert_eq!(m.version.as_deref(), Some("3.2.1"));
        assert!(m.loader.is_none());

        // 非 jar 文件 → 全空降级
        let path = dir.join(format!("piemcl-parse-{}.jar", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"not a jar").unwrap();
        let m = parse_jar(&path);
        std::fs::remove_file(&path).ok();
        assert!(m.mod_id.is_none() && m.name.is_none());
    }
}
