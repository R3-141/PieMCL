// 整合包导入 / 导出（.mrpack / CurseForge zip）
//
// 导入：解析 modrinth.index.json（mrpack）或 manifest.json（CurseForge）→
//       预览（名称 / MC 版本 / 加载器 / 模组数）→ 创建实例 → 下载 files →
//       解压 overrides 到实例 minecraft/（命令层编排，见 commands/pack.rs）
// 导出：收集 Modrinth 来源模组（files 清单）+ 本地模组与 config 等进 overrides
//       → 生成 modrinth.index.json + zip（.mrpack）
// 约定：saves / logs / crash-reports / versions / libraries / assets / natives
//       等运行时产物不进导出包。

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// 包内 overrides 前缀（mrpack 与 CF zip 一致）
const OVERRIDES_PREFIX: &str = "overrides/";

/// 预览信息（导入确认对话框展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPreview {
    /// modrinth / curseforge
    pub kind: String,
    pub name: String,
    pub mc_version: String,
    /// vanilla / fabric / quilt / forge / neoforge
    pub loader_type: String,
    pub loader_version: Option<String>,
    /// 需下载的模组文件数
    pub mod_count: usize,
}

/// 包内单个下载文件（mrpack files）
#[derive(Debug, Clone)]
pub struct PackFile {
    /// 相对实例 minecraft/ 的路径（如 mods/x.jar）
    pub path: String,
    pub urls: Vec<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

/// 解析结果（命令层按 kind 分支处理）
pub enum ParsedPack {
    Modrinth {
        preview: PackPreview,
        files: Vec<PackFile>,
    },
    CurseForge {
        preview: PackPreview,
        /// (projectID, fileID)，下载链接需经 CurseForge API 解析
        files: Vec<(i64, i64)>,
    },
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 打开 zip 并读取文本条目
fn read_zip_text(path: &Path, name: &str) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开整合包失败：{e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("读取整合包 zip 失败：{e}"))?;
    let mut entry = archive
        .by_name(name)
        .map_err(|_| format!("整合包缺少 {name}，不是有效的整合包文件"))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("读取 {name} 失败：{e}"))?;
    Ok(text)
}

/// mrpack 的 modrinth.index.json 结构（宽松解析）
#[derive(Debug, Deserialize)]
pub struct MrpackIndex {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub dependencies: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
}

#[derive(Debug, Deserialize)]
pub struct MrpackFile {
    pub path: String,
    #[serde(default)]
    pub hashes: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub downloads: Vec<String>,
    #[serde(default)]
    pub file_size: u64,
}

/// CurseForge manifest.json 结构（宽松解析）
#[derive(Debug, Deserialize)]
pub struct CfManifest {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub minecraft: serde_json::Value,
    #[serde(default)]
    pub files: Vec<CfPackFile>,
}

#[derive(Debug, Deserialize)]
pub struct CfPackFile {
    #[serde(rename = "projectID")]
    pub project_id: i64,
    #[serde(rename = "fileID")]
    pub file_id: i64,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

/// 解析整合包（自动识别 mrpack / CurseForge zip）
pub fn parse_pack(path: &Path) -> Result<ParsedPack, String> {
    // 优先尝试 mrpack
    if let Ok(text) = read_zip_text(path, "modrinth.index.json") {
        let index: MrpackIndex =
            serde_json::from_str(&text).map_err(|e| format!("解析 modrinth.index.json 失败：{e}"))?;
        let mc_version = index.dependencies.get("minecraft").cloned().unwrap_or_default();
        let (loader_type, loader_version) =
            loader_from_deps(&index.dependencies);
        let mod_count = index
            .files
            .iter()
            .filter(|f| f.path.starts_with("mods/") && f.env.get("client").map(|c| c != "unsupported").unwrap_or(true))
            .count();
        let files = index
            .files
            .iter()
            .filter(|f| f.env.get("client").map(|c| c != "unsupported").unwrap_or(true))
            .map(|f| PackFile {
                path: f.path.clone(),
                urls: f.downloads.clone(),
                sha1: f.hashes.get("sha1").cloned(),
                size: if f.file_size > 0 { Some(f.file_size) } else { None },
            })
            .collect();
        return Ok(ParsedPack::Modrinth {
            preview: PackPreview {
                kind: "modrinth".into(),
                name: index.name,
                mc_version,
                loader_type,
                loader_version,
                mod_count,
            },
            files,
        });
    }

    // 尝试 CurseForge manifest.json
    if let Ok(text) = read_zip_text(path, "manifest.json") {
        let manifest: CfManifest =
            serde_json::from_str(&text).map_err(|e| format!("解析 manifest.json 失败：{e}"))?;
        let mc_version = manifest
            .minecraft
            .get("version")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        let (loader_type, loader_version) = manifest
            .minecraft
            .get("modLoaders")
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.first())
            .and_then(|l| l.get("id").and_then(|x| x.as_str()))
            .map(|id| {
                // 形如 "fabric-0.16.9" / "forge-47.2.0" / "neoforge-20.4.x"
                match id.split_once('-') {
                    Some((t, v)) => (t.to_string(), Some(v.to_string())),
                    None => ("vanilla".to_string(), None),
                }
            })
            .unwrap_or_else(|| ("vanilla".to_string(), None));
        let files: Vec<(i64, i64)> = manifest
            .files
            .iter()
            .filter(|f| f.required)
            .map(|f| (f.project_id, f.file_id))
            .collect();
        return Ok(ParsedPack::CurseForge {
            preview: PackPreview {
                kind: "curseforge".into(),
                name: manifest.name,
                mc_version,
                loader_type,
                loader_version,
                mod_count: files.len(),
            },
            files,
        });
    }

    Err("不是有效的整合包：缺少 modrinth.index.json 或 manifest.json".into())
}

/// 依赖表 → (loader_type, version)
fn loader_from_deps(deps: &std::collections::HashMap<String, String>) -> (String, Option<String>) {
    for key in ["fabric-loader", "quilt-loader"] {
        if let Some(v) = deps.get(key) {
            return (key.trim_end_matches("-loader").to_string(), Some(v.clone()));
        }
    }
    if let Some(v) = deps.get("neoforge") {
        return ("neoforge".into(), Some(v.clone()));
    }
    if let Some(v) = deps.get("forge") {
        return ("forge".into(), Some(v.clone()));
    }
    ("vanilla".into(), None)
}

/// 解压 overrides/ 到实例 minecraft 目录（同名覆盖；跳过实例根配置文件）
pub fn extract_overrides(path: &Path, minecraft_dir: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开整合包失败：{e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("读取整合包 zip 失败：{e}"))?;
    let mut n = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败：{e}"))?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let Ok(sub) = rel.strip_prefix(OVERRIDES_PREFIX) else { continue };
        if sub.as_os_str().is_empty() {
            continue;
        }
        let out = minecraft_dir.join(sub);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("创建目录失败：{e}"))?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p).map_err(|e| format!("创建目录失败：{e}"))?;
            }
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("读取 zip 条目失败：{e}"))?;
            std::fs::write(&out, &buf).map_err(|e| format!("写入 {} 失败：{e}", sub.display()))?;
            n += 1;
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// 导出（.mrpack）
// ---------------------------------------------------------------------------

/// 导出配置
pub struct ExportInput {
    pub instance_name: String,
    pub instance_id: String,
    pub mc_version: String,
    pub loader_type: String,
    pub loader_version: Option<String>,
    /// (相对 minecraft/ 的路径, 绝对路径, sha1 可选, 下载 URL 可选, 文件大小)
    /// 来自 mods.json 的 Modrinth 来源模组
    pub hosted_files: Vec<(String, std::path::PathBuf, Option<String>, Option<String>, u64)>,
    /// 进入 overrides 的本地内容：(zip 内相对 overrides/ 的路径, 绝对路径)
    pub override_files: Vec<(String, std::path::PathBuf)>,
}

/// 生成 .mrpack（modrinth.index.json + overrides/）
pub fn export_mrpack(input: &ExportInput, dest: &Path) -> Result<(), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    // manifest
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), serde_json::json!(input.mc_version));
    match input.loader_type.as_str() {
        "fabric" => {
            if let Some(v) = &input.loader_version {
                deps.insert("fabric-loader".into(), serde_json::json!(v));
            }
        }
        "quilt" => {
            if let Some(v) = &input.loader_version {
                deps.insert("quilt-loader".into(), serde_json::json!(v));
            }
        }
        "forge" => {
            if let Some(v) = &input.loader_version {
                deps.insert("forge".into(), serde_json::json!(v));
            }
        }
        "neoforge" => {
            if let Some(v) = &input.loader_version {
                deps.insert("neoForge".into(), serde_json::json!(v));
            }
        }
        _ => {}
    }
    let index = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": format!("{}-{}", input.instance_name, input.instance_id),
        "name": input.instance_name,
        "files": input.hosted_files.iter().map(|(path, _, sha1, url, size)| serde_json::json!({
            "path": path,
            "hashes": sha1.as_ref().map(|s| serde_json::json!({ "sha1": s })).unwrap_or(serde_json::json!({})),
            "env": { "client": "required", "server": "unsupported" },
            "downloads": [url],
            "fileSize": size,
        })).collect::<Vec<_>>(),
        "dependencies": deps,
    });
    let index_text = serde_json::to_string_pretty(&index)
        .map_err(|e| format!("序列化整合包清单失败：{e}"))?;

    let file = std::fs::File::create(dest).map_err(|e| format!("创建导出文件失败：{e}"))?;
    let mut w = zip::ZipWriter::new(file);
    let opts = SimpleFileOptions::default();
    w.start_file("modrinth.index.json", opts)
        .map_err(|e| format!("写入 zip 失败：{e}"))?;
    w.write_all(index_text.as_bytes())
        .map_err(|e| format!("写入 zip 失败：{e}"))?;
    for (rel, abs) in &input.override_files {
        let data = std::fs::read(abs).map_err(|e| format!("读取 {} 失败：{e}", abs.display()))?;
        w.start_file(format!("{OVERRIDES_PREFIX}{rel}"), opts)
            .map_err(|e| format!("写入 zip 失败：{e}"))?;
        w.write_all(&data).map_err(|e| format!("写入 zip 失败：{e}"))?;
    }
    w.finish()
        .map_err(|e| format!("完成 zip 失败：{e}"))?;
    Ok(())
}

/// 计算文件 SHA-1（小写 hex）
pub fn sha1_file(path: &Path) -> Result<String, String> {
    use sha1::Digest;
    let mut file = std::fs::File::open(path).map_err(|e| format!("打开文件失败：{e}"))?;
    let mut hasher = sha1::Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("读取文件失败：{e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 导出时收集 overrides 的白名单（相对 minecraft/ 的顶层条目）
pub const EXPORT_INCLUDE_DIRS: &[&str] = &[
    "config",
    "resourcepacks",
    "shaderpacks",
    "kubejs",
    "defaultconfigs",
    "scripts",
    "structures",
];
pub const EXPORT_INCLUDE_FILES: &[&str] = &["options.txt", "servers.dat"];

/// 收集 minecraft/ 内需进 overrides 的文件（相对路径, 绝对路径）
pub fn collect_override_files(mc_dir: &Path, extra: Vec<(String, std::path::PathBuf)>) -> Vec<(String, std::path::PathBuf)> {
    let mut out: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut push_file = |rel: String, abs: std::path::PathBuf| {
        if abs.is_file() {
            out.push((rel, abs));
        }
    };
    for dir in EXPORT_INCLUDE_DIRS {
        let root = mc_dir.join(dir);
        collect_recursive(dir, &root, &mut push_file);
    }
    for f in EXPORT_INCLUDE_FILES {
        push_file(f.to_string(), mc_dir.join(f));
    }
    out.extend(extra);
    out
}

fn collect_recursive(
    prefix: &str,
    dir: &Path,
    push: &mut impl FnMut(String, std::path::PathBuf),
) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        if meta.is_file() {
            push(rel, e.path());
        } else if meta.is_dir() {
            collect_recursive(&rel, &e.path(), push);
        }
    }
}
