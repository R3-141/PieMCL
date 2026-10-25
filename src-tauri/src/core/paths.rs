// Pie MCL 目录布局管理
//
// 双根结构（对齐主流启动器，PCL 风格）：
//   launcher_dir（启动器私有，root）：
//     release = exe 同目录 PieMCL\，debug = %APPDATA%/com.piemcl.launcher
//     存放 settings.json / accounts.json / skins / background / java / cache / logs
//   game_root（游戏公共根，.minecraft）：
//     release = exe 同目录 .minecraft\，debug = {root}\.minecraft
//     布局遵循 Mojang 官方规范：
//       assets\ libraries\                 全体实例共享
//       versions\{实例名}\                 实例 = 版本文件夹（版本隔离）
//         {实例名}.jar / {实例名}.json     主 jar 与冻结合并版 version.json（id=实例名）
//         .pie\                            PieMCL 私有元数据（instance.json / loader.json /
//                                          mods.json / trash\ / logs\）
//         saves\ mods\ config\ …           游戏工作目录内容（隔离开启时）
//       launcher_profiles.json             官方规范兼容文件

use std::path::{Path, PathBuf};

/// v2 目录结构迁移完成后写入的标记文件（避免重复迁移）。
/// 注意与批次三便携化迁移的旧标记 ".piemcl-migrated" 区分——旧标记的存在
/// 不代表已完成 v2 迁移，故此处启用全新的标记名。
pub const MIGRATED_MARKER: &str = ".piemcl-v2-migrated";

/// 旧布局检测：根目录下存在 instances\（v1 每实例独立目录结构）
pub fn is_old_layout(root: &Path) -> bool {
    root.join("instances").is_dir()
}

/// 实例名 → 合法且唯一的版本文件夹名。
/// 剔除 Windows 非法字符与首尾空白/点，超长截断；重名自动追加 -2 / -3 …
pub fn sanitize_folder_name(name: &str) -> String {
    let mut s: String = name
        .trim()
        .chars()
        .filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    // Windows 文件名不能以点/空格结尾
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let s: String = s.chars().take(60).collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "instance".to_string()
    } else {
        s
    }
}

/// 在 versions 根下生成不冲突的文件夹名
pub fn unique_folder_name(versions_dir: &Path, name: &str) -> String {
    let base = sanitize_folder_name(name);
    if !versions_dir.join(&base).exists() {
        return base;
    }
    for i in 2..1000 {
        let candidate = format!("{base}-{i}");
        if !versions_dir.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{}-{}", base, &uuid::Uuid::new_v4().simple().to_string()[..6])
}

// ---------------------------------------------------------------------------
// 旧布局迁移（v1 → v2）
// ---------------------------------------------------------------------------

/// 递归合并 src 目录内容到 dst（同名文件保留 dst 侧，即"已存在优先"；
/// 缺失项优先 rename（同卷零拷贝），跨卷回落 copy）
fn merge_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            merge_dir_recursive(&from, &to)?;
        } else if !to.exists() {
            if std::fs::rename(&from, &to).is_err() {
                std::fs::copy(&from, &to)?;
            }
        }
    }
    Ok(())
}

/// 递归移动目录内容到 dst（move 优先，跨盘回落 copy+delete）
fn move_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            move_dir_recursive(&from, &to)?;
            let _ = std::fs::remove_dir(&from); // 空目录清理，非空则保留
        } else if !to.exists() {
            if std::fs::rename(&from, &to).is_err() {
                std::fs::copy(&from, &to)?;
                let _ = std::fs::remove_file(&from);
            }
        }
    }
    Ok(())
}

/// v1 → v2 迁移（staging 原子性由调用方保证先备份；此处逐步移动，失败时
/// 已移动内容保留 —— 各步骤均为幂等 move/merge，修复原因后重跑即可续迁）：
/// 1. 启动器私有文件（java/skins/background/cache/settings/accounts）→ launcher_dir
/// 2. 逐实例（instances/{id}）：
///    - minecraft\assets、libraries 合并进 game_root（共享）
///    - minecraft\ 其余内容（saves/mods/config/options.txt 等）→ game_root/versions/{实例名}\
///    - minecraft\versions\{mc}\{mc}.jar → versions/{实例名}/{实例名}.jar
///    - instance.json + version.json → versions/{实例名}/.pie\（version.json 改名
///      {实例名}.json 置于版本文件夹根，id 字段改写为实例名）
/// 3. 删除空的 instances\
pub fn migrate_v2(old_root: &Path, launcher_dir: &Path, game_root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(launcher_dir)?;
    std::fs::create_dir_all(game_root)?;

    // ---- 1. 启动器私有文件归位（旧根与新启动器目录同址时跳过，已在位）----
    if launcher_dir != old_root {
        for item in ["java", "skins", "background", "cache"] {
            let src = old_root.join(item);
            if src.is_dir() {
                move_dir_recursive(&src, &launcher_dir.join(item))?;
                let _ = std::fs::remove_dir_all(&src);
            }
        }
        for file in ["settings.json", "accounts.json"] {
            let src = old_root.join(file);
            let dst = launcher_dir.join(file);
            if src.is_file() && !dst.exists() {
                std::fs::rename(&src, &dst).or_else(|_| std::fs::copy(&src, &dst).map(|_| ()))?;
            }
        }
    }

    // ---- 2. 实例重组 ----
    let instances = old_root.join("instances");
    let versions = game_root.join("versions");
    if instances.is_dir() {
        std::fs::create_dir_all(&versions)?;
        for entry in std::fs::read_dir(&instances)? {
            let entry = entry?;
            let inst = entry.path();
            if !inst.is_dir() {
                continue;
            }
            migrate_instance_v1(&inst, &versions)?;
        }
        let _ = std::fs::remove_dir_all(&instances);
    }
    Ok(())
}

/// 迁移单个 v1 实例目录到新版本文件夹结构
fn migrate_instance_v1(inst: &Path, versions: &Path) -> std::io::Result<()> {
    // 实例名：优先 instance.json 的 name，缺失用目录名
    let name = std::fs::read_to_string(inst.join("instance.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
        .unwrap_or_else(|| {
            inst.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let folder = unique_folder_name(versions, &name);
    let dest = versions.join(&folder);
    std::fs::create_dir_all(dest.join(".pie"))?;

    let mc = inst.join("minecraft");
    if mc.is_dir() {
        // assets / libraries → 游戏根（共享合并）
        for shared in ["assets", "libraries"] {
            let src = mc.join(shared);
            if src.is_dir() {
                merge_dir_recursive(&src, &game_root_of(versions).join(shared))?;
                let _ = std::fs::remove_dir_all(&src);
            }
        }
        // 版本 jar：versions/{mc}/{mc}.jar → {folder}/{folder}.jar
        //（可能存在多个 MC 版本子目录，逐个搬 jar/json；游戏工作目录内容一并归位）
        let mc_versions = mc.join("versions");
        if mc_versions.is_dir() {
            for entry in std::fs::read_dir(&mc_versions)? {
                let entry = entry?;
                let vdir = entry.path();
                if !vdir.is_dir() {
                    continue;
                }
                // jar 归位（只处理与冻结 version.json id 对应的那份，其余忽略）
                if let Ok(vtext) = std::fs::read_to_string(inst.join("version.json")) {
                    if let Ok(vjson) = serde_json::from_str::<serde_json::Value>(&vtext) {
                        if let Some(id) = vjson.get("id").and_then(|x| x.as_str()) {
                            if vdir.file_name().map(|n| n == id).unwrap_or(false) {
                                let jar = vdir.join(format!("{id}.jar"));
                                if jar.is_file() {
                                    std::fs::rename(&jar, dest.join(format!("{folder}.jar")))?;
                                }
                            }
                        }
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&mc_versions);
        }
        // 其余内容（saves/mods/config/options.txt/logs…）→ 版本文件夹（版本隔离）
        move_dir_recursive(&mc, &dest)?;
        let _ = std::fs::remove_dir_all(&mc);
    }

    // 元数据：version.json → {folder}.json（id 改写）；instance.json / loader.json → .pie\
    let vjson_path = inst.join("version.json");
    if vjson_path.is_file() {
        let mut vjson: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&vjson_path).unwrap_or_else(|_| "{}".into()),
        )
        .unwrap_or(serde_json::json!({}));
        if let Some(obj) = vjson.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!(folder));
        }
        std::fs::write(
            dest.join(format!("{folder}.json")),
            serde_json::to_string_pretty(&vjson).unwrap_or_default(),
        )?;
        let _ = std::fs::remove_file(&vjson_path);
    }
    for meta in ["instance.json", "loader.json"] {
        let src = inst.join(meta);
        if src.is_file() {
            let _ = std::fs::rename(&src, dest.join(".pie").join(meta));
        }
    }
    // 启动器日志 → .pie\logs\
    let logs = inst.join("logs");
    if logs.is_dir() {
        let _ = std::fs::rename(&logs, dest.join(".pie").join("logs"));
    }
    let _ = std::fs::remove_dir_all(inst);
    Ok(())
}

/// 由 versions 路径反推游戏根（versions 的父目录）
fn game_root_of(versions: &Path) -> PathBuf {
    versions.parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// 启动器路径集合：root = 启动器私有目录，game_root = 游戏公共根
#[derive(Debug, Clone)]
pub struct LauncherPaths {
    root: PathBuf,
    game_root: PathBuf,
}

impl LauncherPaths {
    pub fn new(root: PathBuf, game_root: PathBuf) -> Self {
        Self { root, game_root }
    }

    /// 启动器私有目录（accounts.json 等根级文件使用）
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 游戏公共根（.minecraft）：assets / libraries / versions
    pub fn game_root(&self) -> &Path {
        &self.game_root
    }

    /// 版本总目录：{game_root}/versions
    pub fn versions_root(&self) -> PathBuf {
        self.game_root.join("versions")
    }

    /// 实例的版本文件夹（实例 id 即文件夹名）：
    /// {game_root}/versions/{id}，含 {id}.jar / {id}.json / .pie\ 与游戏文件
    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.versions_root().join(id)
    }

    /// 实例私有元数据目录：{版本文件夹}/.pie
    pub fn instance_meta_dir(&self, id: &str) -> PathBuf {
        self.instance_dir(id).join(".pie")
    }

    /// 实例冻结的版本元数据：{版本文件夹}/{id}.json（官方规范命名）
    pub fn instance_version_json(&self, id: &str) -> PathBuf {
        self.instance_dir(id).join(format!("{id}.json"))
    }

    /// 实例游戏工作目录：隔离开启 = 版本文件夹；关闭 = .minecraft 根
    pub fn instance_game_dir(&self, id: &str, isolation: bool) -> PathBuf {
        if isolation {
            self.instance_dir(id)
        } else {
            self.game_root.clone()
        }
    }

    /// 启动器日志目录（.{实例}/.pie/logs，区别于游戏自身 logs\）
    pub fn instance_logs_dir(&self, id: &str) -> PathBuf {
        self.instance_meta_dir(id).join("logs")
    }

    /// 全局缓存目录
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// 版本 JSON 缓存根目录
    pub fn versions_cache_dir(&self) -> PathBuf {
        self.cache_dir().join("versions")
    }

    /// 指定版本 JSON 的缓存文件路径
    pub fn version_json_cache(&self, id: &str) -> PathBuf {
        self.versions_cache_dir().join(id).join(format!("{id}.json"))
    }

    /// 版本清单缓存文件
    pub fn manifest_cache(&self) -> PathBuf {
        self.cache_dir().join("version_manifest_v2.json")
    }

    /// 版本清单缓存元信息（fetched_at / source）
    pub fn manifest_meta(&self) -> PathBuf {
        self.cache_dir().join("version_manifest_v2.meta.json")
    }

    /// 下载临时目录（分块 .part 文件与断点 .meta.json）
    pub fn tmp_dir(&self) -> PathBuf {
        self.cache_dir().join("tmp")
    }

    /// 全局设置文件
    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// 自定义背景图目录
    pub fn background_dir(&self) -> PathBuf {
        self.root.join("background")
    }

    /// 自定义背景图文件（ext：jpg / png / webp）
    pub fn background_path(&self, ext: &str) -> PathBuf {
        self.background_dir().join(format!("bg.{ext}"))
    }

    /// 自动下载的 Java 运行时根目录
    #[allow(dead_code)] // 批次 C（Java 管理）使用
    pub fn java_dir(&self) -> PathBuf {
        self.root.join("java")
    }

    /// 联机（樱花内网穿透）目录：frpc 可执行文件与配置
    pub fn natfrp_dir(&self) -> PathBuf {
        self.root.join("natfrp")
    }

    /// 樱花 frpc 配置文件
    pub fn natfrp_config(&self) -> PathBuf {
        self.natfrp_dir().join("config.json")
    }

    /// frpc 可执行文件默认路径（用户可手动指定覆盖）
    pub fn natfrp_frpc_path(&self) -> PathBuf {
        self.natfrp_dir().join("frpc.exe")
    }

    /// 确保基础目录存在（启动时调用一次）
    pub async fn ensure_dirs(&self) -> std::io::Result<()> {
        tokio::fs::create_dir_all(self.versions_root()).await?;
        tokio::fs::create_dir_all(self.versions_cache_dir()).await?;
        tokio::fs::create_dir_all(self.tmp_dir()).await?;
        tokio::fs::create_dir_all(self.java_dir()).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_and_unique_names() {
        assert_eq!(sanitize_folder_name("  我的 整合包  "), "我的 整合包");
        assert_eq!(sanitize_folder_name("a/b\\c:d*e?f\"g<h>i|j"), "abcdefghij");
        assert_eq!(sanitize_folder_name("结尾点."), "结尾点");
        assert_eq!(sanitize_folder_name("///"), "instance");
        let dir = std::env::temp_dir().join(format!("piemcl-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Foo")).unwrap();
        assert_eq!(unique_folder_name(&dir, "Foo"), "Foo-2");
        assert_eq!(unique_folder_name(&dir, "Bar"), "Bar");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_moves_data_and_skips_cache() {
        let base = std::env::temp_dir().join(format!("piemcl-mig2-{}", std::process::id()));
        let old = base.join("old");
        let launcher = base.join("PieMCL");
        let game = base.join(".minecraft");
        // v1 布局 fixture
        std::fs::create_dir_all(old.join("instances/a1/minecraft/versions/1.20.1")).unwrap();
        std::fs::create_dir_all(old.join("instances/a1/minecraft/saves/w")).unwrap();
        std::fs::create_dir_all(old.join("instances/a1/minecraft/assets/objects/xx")).unwrap();
        std::fs::create_dir_all(old.join("cache/tmp")).unwrap();
        std::fs::write(old.join("cache/tmp/part.bin"), b"p").unwrap();
        std::fs::write(
            old.join("instances/a1/instance.json"),
            r#"{"id":"a1","name":"生存服","mcVersion":"1.20.1","createdAt":1}"#,
        )
        .unwrap();
        std::fs::write(
            old.join("instances/a1/version.json"),
            r#"{"id":"1.20.1","mainClass":"Main"}"#,
        )
        .unwrap();
        std::fs::write(old.join("instances/a1/minecraft/versions/1.20.1/1.20.1.jar"), b"j").unwrap();
        std::fs::write(old.join("instances/a1/minecraft/saves/w/level.dat"), b"s").unwrap();
        std::fs::write(old.join("instances/a1/minecraft/assets/objects/xx/xxyy"), b"a").unwrap();
        std::fs::write(old.join("instances/a1/minecraft/options.txt"), b"lang:en_us").unwrap();
        std::fs::write(old.join("settings.json"), b"{}").unwrap();

        assert!(is_old_layout(&old));
        migrate_v2(&old, &launcher, &game).unwrap();

        // 启动器文件归位
        assert!(launcher.join("settings.json").is_file());
        assert!(launcher.join("cache/tmp/part.bin").is_file());
        // 实例重组：版本文件夹按实例名命名
        let dest = game.join("versions/生存服");
        assert!(dest.join("生存服.jar").is_file(), "主 jar 应改名归位");
        assert!(dest.join("生存服.json").is_file(), "version.json 应改名归位");
        assert!(dest.join(".pie/instance.json").is_file());
        assert!(dest.join("saves/w/level.dat").is_file(), "游戏文件应进版本文件夹");
        assert!(dest.join("options.txt").is_file());
        // version.json id 改写为文件夹名
        let vjson: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dest.join("生存服.json")).unwrap())
                .unwrap();
        assert_eq!(vjson["id"], "生存服");
        // assets 合并进游戏根
        assert!(game.join("assets/objects/xx/xxyy").is_file());
        // 旧目录应清空
        assert!(!old.join("instances").exists());
        assert!(!old.join("cache").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
