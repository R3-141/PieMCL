// 本地社区资源管理：扫描 / 启停 / 回收站 / 拖放复制
//
// 目录约定（全部限定在单实例内，禁止跨实例污染）：
//   {版本文件夹}/mods/*.jar                    模组（启用）
//   {版本文件夹}/mods/*.jar.disabled           模组（禁用，重命名实现，不移动文件）
//   {版本文件夹}/datapacks/*.zip               数据包（落根目录，世界启用由用户在游戏中操作）
//   {版本文件夹}/shaderpacks/*.zip             光影包
//   {版本文件夹}/resourcepacks/*.zip           资源包
//   （以上目录以游戏工作目录为根：隔离开启 = versions/{实例名}，关闭 = .minecraft 根）
//   {版本文件夹}/.pie/trash/{删除时间ms}_{原名}  回收站（实例级共用，保留 N 天，0 = 永久删除）
//   {版本文件夹}/.pie/mods.json                来源记录（见 store.rs，key 带类型前缀）
//
// 能力差异：启停与依赖解析仅模组支持；数据包 / 光影 / 资源包
// 仅做文件管理（删除 / 恢复 / 来源记录），不解析内容。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::parse::parse_jar;
use super::store::{ModSource, ModSources};
use super::ResourceType;

/// 单个资源文件的展示信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModInfo {
    /// 文件名（含 .disabled 后缀，模组启停操作凭据）
    pub file_name: String,
    pub enabled: bool,
    pub file_size: u64,
    /// 元数据 id；解析失败为 None（降级为文件名展示；非模组恒 None）
    pub mod_id: Option<String>,
    /// 显示名（解析失败 = 文件名去后缀）
    pub name: String,
    pub version: Option<String>,
    /// 元数据自报加载器：fabric / forge / neoforge / None
    pub loader: Option<String>,
    /// 必需前置依赖 id（仅模组）
    pub depends: Vec<String>,
    /// 缺失的前置（depends − 已启用模组 id 集合，仅模组）
    pub missing_deps: Vec<String>,
    /// 来源记录（拖放安装为 local；无记录 = 外部放入的旧文件）
    pub source: Option<ModSource>,
}

/// 回收站条目
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    /// 回收站内的完整文件名（{时间ms}_{原名}）
    pub trash_name: String,
    pub original_name: String,
    /// 删除时间（epoch 秒，取自文件名前缀）
    pub deleted_at: i64,
    pub file_size: u64,
}

/// 类型对应的落位目录
pub fn resource_dir(mc_dir: &Path, kind: ResourceType) -> PathBuf {
    mc_dir.join(kind.dir_name())
}

pub fn trash_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join(".pie").join("trash")
}

/// 文件名 → 来源记录键基名（去 .disabled 后缀）
fn base_name(file_name: &str) -> &str {
    file_name.strip_suffix(".disabled").unwrap_or(file_name)
}

/// 扫描类型对应目录；模组额外解析元数据与计算缺失依赖；
/// 顺带惰性清理过期回收站。非模组类型不解析内容（enabled 恒 true）。
pub fn scan_mods(
    instance_dir: &Path,
    mc_dir: &Path,
    retention_days: u32,
    kind: ResourceType,
) -> Vec<ModInfo> {
    cleanup_trash(instance_dir, retention_days);

    let dir = resource_dir(mc_dir, kind);
    let ext = kind.file_ext();
    let sources = ModSources::load(instance_dir);

    if !kind.is_mod() {
        // 数据包 / 光影 / 资源包：仅列文件，不解析内容
        let mut files: Vec<(String, u64)> = Vec::new();
        if let Ok(read) = std::fs::read_dir(&dir) {
            for e in read.flatten() {
                let Ok(meta) = e.metadata() else { continue };
                if !meta.is_file() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(ext) {
                    files.push((name, meta.len()));
                }
            }
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        return files
            .into_iter()
            .map(|(name, size)| {
                let base = name.trim_end_matches(ext).to_string();
                let source = sources.get(kind, &base).cloned();
                ModInfo {
                    file_name: name,
                    enabled: true,
                    file_size: size,
                    mod_id: None,
                    name: base,
                    version: None,
                    loader: None,
                    depends: Vec::new(),
                    missing_deps: Vec::new(),
                    source,
                }
            })
            .collect();
    }

    // 模组：接受 .jar 与 .jar.disabled
    let mut entries: Vec<(String, bool, u64)> = Vec::new(); // (文件名, 启用, 大小)
    if let Ok(read) = std::fs::read_dir(&dir) {
        for e in read.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let enabled = if name.ends_with(".jar.disabled") {
                false
            } else if name.ends_with(".jar") {
                true
            } else {
                continue; // 仅接受 jar 与 jar.disabled
            };
            entries.push((name, enabled, meta.len()));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // 第一轮：逐个解析元数据
    let mut mods: Vec<ModInfo> = entries
        .iter()
        .map(|(name, enabled, size)| {
            let path = dir.join(name);
            let parsed = parse_jar(&path);
            let fallback = base_name(name).trim_end_matches(".jar").to_string();
            ModInfo {
                file_name: name.clone(),
                enabled: *enabled,
                file_size: *size,
                mod_id: parsed.mod_id,
                name: parsed.name.unwrap_or(fallback),
                version: parsed.version,
                loader: parsed.loader,
                depends: parsed.depends,
                missing_deps: Vec::new(),
                source: sources.get(kind, base_name(name)).cloned(),
            }
        })
        .collect();

    // 第二轮：缺失依赖 = depends − 已启用模组 id 集合
    let enabled_ids: HashSet<String> = mods
        .iter()
        .filter(|m| m.enabled)
        .filter_map(|m| m.mod_id.clone())
        .collect();
    for m in &mut mods {
        if !m.enabled {
            continue;
        }
        m.missing_deps = m
            .depends
            .iter()
            .filter(|d| !enabled_ids.contains(*d))
            .cloned()
            .collect();
    }
    mods
}

/// 启用 / 禁用（同目录重命名，不移动文件）。仅模组支持。
pub fn toggle_mod(
    mc_dir: &Path,
    kind: ResourceType,
    file_name: &str,
    enable: bool,
) -> Result<(), String> {
    if !kind.is_mod() {
        return Err(format!("{}不支持启用/禁用操作", kind.label()));
    }
    validate_file_name(file_name, kind)?;
    // 列表返回的禁用态文件名含 .disabled 后缀，剥成基名再构造重命名对
    let base = file_name.strip_suffix(".disabled").unwrap_or(file_name);
    let dir = resource_dir(mc_dir, kind);
    let (from, to) = match enable {
        // 禁用：追加 .disabled（目标已存在 = 状态异常，报错而非覆盖）
        true => (format!("{base}.disabled"), base.to_string()),
        false => (base.to_string(), format!("{base}.disabled")),
    };
    let from = dir.join(&from);
    let to = dir.join(&to);
    if !from.is_file() {
        return Err(format!("文件不存在：{}", from.display()));
    }
    if to.exists() {
        return Err(format!("目标文件已存在，状态可能已被外部修改：{}", to.display()));
    }
    std::fs::rename(&from, &to).map_err(|e| format!("重命名失败：{e}"))
}

/// 删除到回收站；返回成功移除的文件名。retention_days=0 直接永久删除。
pub fn delete_mods(
    instance_dir: &Path,
    mc_dir: &Path,
    kind: ResourceType,
    file_names: &[String],
    retention_days: u32,
) -> Result<Vec<String>, String> {
    let dir = resource_dir(mc_dir, kind);
    let mut sources = ModSources::load(instance_dir);
    let mut removed = Vec::new();
    for name in file_names {
        validate_file_name(name, kind)?;
        let path = dir.join(name);
        if !path.is_file() {
            return Err(format!("文件不存在：{}", path.display()));
        }
        if retention_days == 0 {
            std::fs::remove_file(&path).map_err(|e| format!("删除 {name} 失败：{e}"))?;
        } else {
            let trash = trash_dir(instance_dir);
            std::fs::create_dir_all(&trash)
                .map_err(|e| format!("创建回收站目录失败：{e}"))?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let dest = trash.join(format!("{stamp}_{name}"));
            std::fs::rename(&path, &dest).map_err(|e| format!("移入回收站失败：{e}"))?;
        }
        sources.remove(kind, base_name(name));
        removed.push(name.clone());
    }
    sources.save(instance_dir)?;
    Ok(removed)
}

/// 列出回收站条目（实例级共用，各资源页共享同一回收站）
pub fn list_trash(instance_dir: &Path) -> Vec<TrashItem> {
    let trash = trash_dir(instance_dir);
    let mut out = Vec::new();
    if let Ok(read) = std::fs::read_dir(&trash) {
        for e in read.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let trash_name = e.file_name().to_string_lossy().into_owned();
            // 文件名格式 {epoch_ms}_{原名}
            let (deleted_at, original_name) = match trash_name.split_once('_') {
                Some((t, rest)) => match t.parse::<i64>() {
                    Ok(ms) => (ms / 1000, rest.to_string()),
                    Err(_) => (0, trash_name.clone()),
                },
                None => (0, trash_name.clone()),
            };
            out.push(TrashItem {
                trash_name,
                original_name,
                deleted_at,
                file_size: meta.len(),
            });
        }
    }
    out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    out
}

/// 从回收站恢复到类型对应目录（同名已存在时报错；文件后缀须与类型匹配）
pub fn restore_mod(
    instance_dir: &Path,
    mc_dir: &Path,
    kind: ResourceType,
    trash_name: &str,
) -> Result<(), String> {
    if trash_name.contains("..") || trash_name.contains('/') || trash_name.contains('\\') {
        return Err("非法的回收站文件名".into());
    }
    let src = trash_dir(instance_dir).join(trash_name);
    if !src.is_file() {
        return Err(format!("回收站条目不存在：{trash_name}"));
    }
    let original = trash_name.split_once('_').map(|(_, r)| r).unwrap_or(trash_name);
    // 删除禁用态模组时回收站名含 .disabled 后缀，剥掉后再校验后缀归属
    let base = original.strip_suffix(".disabled").unwrap_or(original);
    if !base.ends_with(kind.file_ext()) {
        return Err(format!(
            "「{original}」不属于{}，请到对应的资源页恢复",
            kind.label()
        ));
    }
    let dest = resource_dir(mc_dir, kind).join(original);
    if dest.exists() {
        return Err(format!(
            "{}目录已存在同名文件：{original}，请先处理冲突",
            kind.label()
        ));
    }
    std::fs::rename(&src, &dest).map_err(|e| format!("恢复失败：{e}"))
}

/// 清空回收站；返回清理的文件数
pub fn empty_trash(instance_dir: &Path) -> Result<usize, String> {
    let trash = trash_dir(instance_dir);
    let mut n = 0;
    if let Ok(read) = std::fs::read_dir(&trash) {
        for e in read.flatten() {
            if e.metadata().map(|m| m.is_file()).unwrap_or(false) {
                std::fs::remove_file(e.path()).map_err(|err| format!("清空回收站失败：{err}"))?;
                n += 1;
            }
        }
    }
    Ok(n)
}

/// 惰性清理：删除超过保留期的回收站条目
pub fn cleanup_trash(instance_dir: &Path, retention_days: u32) {
    if retention_days == 0 {
        return;
    }
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64 - (retention_days as i64) * 86_400_000)
        .unwrap_or(i64::MAX);
    let Ok(read) = std::fs::read_dir(trash_dir(instance_dir)) else {
        return;
    };
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some((prefix, _)) = name.split_once('_') {
            if let Ok(ms) = prefix.parse::<i64>() {
                if ms < cutoff {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

/// 复制外部文件到类型对应目录（拖放 / 从文件安装）。
/// 模组接受 .jar，其余接受 .zip。同名冲突时：overwrite=false 跳过，true 覆盖。
/// 返回 (实际安装名, 是否跳过)。
pub fn install_local_file(
    src: &Path,
    mc_dir: &Path,
    kind: ResourceType,
    overwrite: bool,
) -> Result<(String, bool), String> {
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("非法路径：{}", src.display()))?;
    if !file_name.to_ascii_lowercase().ends_with(kind.file_ext()) {
        return Err(format!(
            "仅支持 {} 文件：{file_name}",
            kind.file_ext()
        ));
    }
    let dest_dir = resource_dir(mc_dir, kind);
    let dest = dest_dir.join(&file_name);
    if dest.exists() && !overwrite {
        return Ok((file_name, true));
    }
    std::fs::create_dir_all(&dest_dir).map_err(|e| {
        format!("创建 {} 目录失败：{e}", kind.dir_name())
    })?;
    std::fs::copy(src, &dest).map_err(|e| format!("复制 {file_name} 失败：{e}"))?;
    Ok((file_name, false))
}

/// 文件名白名单校验：防路径穿越（只允许进入类型根目录）。
/// 模组禁用态文件名带 `.disabled` 后缀（如 a.jar.disabled），剥掉后再校验。
fn validate_file_name(name: &str, kind: ResourceType) -> Result<(), String> {
    let base = name.strip_suffix(".disabled").unwrap_or(name);
    if base.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || !base.ends_with(kind.file_ext())
    {
        return Err(format!("非法的资源文件名：{name}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_instance() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piemcl-mods-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("minecraft").join("mods")).unwrap();
        dir
    }

    #[test]
    fn toggle_accepts_disabled_credential() {
        let inst = temp_instance();
        let mc = inst.join("minecraft");
        std::fs::write(mc.join("mods").join("a.jar.disabled"), b"x").unwrap();

        // 列表返回的禁用态文件名（含 .disabled）可直接再启用（回归：非法的资源文件名）
        toggle_mod(&mc, ResourceType::Mod, "a.jar.disabled", true).unwrap();
        assert!(mc.join("mods").join("a.jar").is_file());
        assert!(!mc.join("mods").join("a.jar.disabled").exists());

        // 启用态凭据再禁用同样正常
        toggle_mod(&mc, ResourceType::Mod, "a.jar", false).unwrap();
        assert!(mc.join("mods").join("a.jar.disabled").is_file());
    }

    #[test]
    fn toggle_and_scan_states() {
        let inst = temp_instance();
        let mc = inst.join("minecraft");
        std::fs::write(mc.join("mods").join("a.jar"), b"x").unwrap();
        std::fs::write(mc.join("mods").join("b.jar.disabled"), b"y").unwrap();

        let mods = scan_mods(&inst, &mc, 7, ResourceType::Mod);
        assert_eq!(mods.len(), 2);
        assert_eq!(mods[0].file_name, "a.jar");
        assert!(mods[0].enabled);
        assert!(!mods[1].enabled);

        // 启用 b：b.jar.disabled → b.jar
        toggle_mod(&mc, ResourceType::Mod, "b.jar", true).unwrap();
        assert!(mc.join("mods").join("b.jar").is_file());
        assert!(!mc.join("mods").join("b.jar.disabled").exists());

        // 目标已存在时报错（不覆盖）
        toggle_mod(&mc, ResourceType::Mod, "a.jar", false).unwrap();
        assert!(toggle_mod(&mc, ResourceType::Mod, "a.jar", false).is_err());
        // 非 mod 类型不支持启停
        assert!(toggle_mod(&mc, ResourceType::Shader, "a.jar", false).is_err());
        // 再禁用回去，恢复初始
        toggle_mod(&mc, ResourceType::Mod, "a.jar", true).unwrap();
        let _ = std::fs::remove_dir_all(&inst);
    }

    #[test]
    fn scan_zip_resources() {
        let inst = temp_instance();
        let mc = inst.join("minecraft");
        std::fs::create_dir_all(mc.join("shaderpacks")).unwrap();
        std::fs::write(mc.join("shaderpacks").join("fancy.zip"), b"s").unwrap();
        std::fs::write(mc.join("shaderpacks").join("note.txt"), b"n").unwrap();

        let shaders = scan_mods(&inst, &mc, 7, ResourceType::Shader);
        assert_eq!(shaders.len(), 1);
        assert_eq!(shaders[0].file_name, "fancy.zip");
        assert_eq!(shaders[0].name, "fancy");
        assert!(shaders[0].enabled);
        assert!(shaders[0].mod_id.is_none());
        let _ = std::fs::remove_dir_all(&inst);
    }

    #[test]
    fn delete_restore_trash_flow() {
        let inst = temp_instance();
        let mc = inst.join("minecraft");
        std::fs::write(mc.join("mods").join("m.jar"), b"data").unwrap();

        // 删除 → 回收站
        let removed = delete_mods(&inst, &mc, ResourceType::Mod, &["m.jar".into()], 7).unwrap();
        assert_eq!(removed, vec!["m.jar"]);
        assert!(!mc.join("mods").join("m.jar").exists());
        let trash = list_trash(&inst);
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].original_name, "m.jar");
        assert!(trash[0].trash_name.ends_with("_m.jar"));

        // 恢复 → 回到 mods/
        restore_mod(&inst, &mc, ResourceType::Mod, &trash[0].trash_name).unwrap();
        assert!(mc.join("mods").join("m.jar").is_file());
        assert!(list_trash(&inst).is_empty());

        // retention=0 → 直接删除
        delete_mods(&inst, &mc, ResourceType::Mod, &["m.jar".into()], 0).unwrap();
        assert!(list_trash(&inst).is_empty());
        let _ = std::fs::remove_dir_all(&inst);
    }

    #[test]
    fn restore_rejects_type_mismatch() {
        let inst = temp_instance();
        let mc = inst.join("minecraft");
        std::fs::create_dir_all(mc.join("datapacks")).unwrap();
        std::fs::write(mc.join("datapacks").join("p.zip"), b"d").unwrap();
        delete_mods(&inst, &mc, ResourceType::Datapack, &["p.zip".into()], 7).unwrap();
        let trash = list_trash(&inst);
        // zip 文件不能恢复到模组页
        assert!(restore_mod(&inst, &mc, ResourceType::Mod, &trash[0].trash_name).is_err());
        restore_mod(&inst, &mc, ResourceType::Datapack, &trash[0].trash_name).unwrap();
        assert!(mc.join("datapacks").join("p.zip").is_file());
        let _ = std::fs::remove_dir_all(&inst);
    }

    #[test]
    fn local_install_skips_conflict() {
        let inst = temp_instance();
        let mc = inst.join("minecraft");
        let src_dir = std::env::temp_dir().join(format!("piemcl-src-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("new.jar");
        std::fs::write(&src, b"v1").unwrap();

        let (name, skipped) = install_local_file(&src, &mc, ResourceType::Mod, false).unwrap();
        assert_eq!(name, "new.jar");
        assert!(!skipped);
        // 冲突：不覆盖则跳过
        let (_, skipped) = install_local_file(&src, &mc, ResourceType::Mod, false).unwrap();
        assert!(skipped);
        // overwrite 覆盖
        std::fs::write(&src, b"v2").unwrap();
        let (_, skipped) = install_local_file(&src, &mc, ResourceType::Mod, true).unwrap();
        assert!(!skipped);
        assert_eq!(std::fs::read(mc.join("mods").join("new.jar")).unwrap(), b"v2");

        // 非 jar 拒绝
        let txt = src_dir.join("readme.txt");
        std::fs::write(&txt, b"hi").unwrap();
        assert!(install_local_file(&txt, &mc, ResourceType::Mod, false).is_err());

        // zip 装到资源包目录
        let zip = src_dir.join("pack.zip");
        std::fs::write(&zip, b"z").unwrap();
        let (name, _) = install_local_file(&zip, &mc, ResourceType::ResourcePack, false).unwrap();
        assert_eq!(name, "pack.zip");
        assert!(mc.join("resourcepacks").join("pack.zip").is_file());
        // zip 不能装进模组目录
        assert!(install_local_file(&zip, &mc, ResourceType::Mod, false).is_err());

        let _ = std::fs::remove_dir_all(&inst);
        let _ = std::fs::remove_dir_all(&src_dir);
    }

    #[test]
    fn file_name_validation_blocks_traversal() {
        assert!(validate_file_name("../../evil.jar", ResourceType::Mod).is_err());
        assert!(validate_file_name("a\\b.jar", ResourceType::Mod).is_err());
        assert!(validate_file_name("noext", ResourceType::Mod).is_err());
        assert!(validate_file_name("ok.jar", ResourceType::Mod).is_ok());
        assert!(validate_file_name("ok.zip", ResourceType::Mod).is_err());
        assert!(validate_file_name("ok.zip", ResourceType::Datapack).is_ok());
    }
}
