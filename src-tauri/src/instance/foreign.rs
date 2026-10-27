// 外部实例扫描导入：开箱即用兼容 HMCL / PCL / 官方启动器的版本文件夹
//
// 原则：零侵入 —— 不移动 / 不修改 / 不覆盖任何原有文件，仅新增 .pie\ 元数据，
// 其他启动器之后仍可正常启动这些实例。导入内容：
//   .pie\instance.json   实例元数据（隔离开启：PCL/HMCL 实例的 saves/mods 本就
//                        在版本文件夹内，与隔离模式天然一致）
//   .pie\loader.json     从 libraries / mainClass 检测加载器（installed=true，
//                        加载器已就位，避免启动时误触发骨架补装）
//   .pie\version.json    冻结的继承链合并版 version json —— 启动管线优先读取
//                        此文件（{id}.json 原样保留给原启动器）
// 解析失败的文件夹写入 .pie\import-failed 标记（含原因），后续扫描跳过；
// 用户修复问题后删除标记即可重试。

use std::path::Path;

use serde_json::Value;

use super::model::{InstanceMeta, JvmSettings, LoaderInfo};
use crate::core::paths::LauncherPaths;
use crate::version::resolve::merge_version_json;

/// 继承链最大深度（与 version::resolve 一致的防御上限）
const MAX_CHAIN_DEPTH: usize = 8;

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 链节点：版本 json 及其来源版本 id
struct ChainNode {
    id: String,
    json: Value,
}

/// 读取链上一节点：本地优先（versions/{id}/{id}.json，PCL/HMCL 的父版本
/// json 就近存放），缺失回落网络仓库
async fn fetch_local_first(
    paths: &LauncherPaths,
    id: &str,
) -> Result<Value, String> {
    let local = paths.versions_root().join(id).join(format!("{id}.json"));
    if let Ok(text) = tokio::fs::read_to_string(&local).await {
        return serde_json::from_str(&text).map_err(|e| format!("解析 {id}.json 失败：{e}"));
    }
    let repo = crate::version::repo::VersionRepo::new(paths.clone());
    repo.fetch_version_json_by_id(id)
        .await
        .map_err(|e| format!("本地缺少父版本 {id}.json，网络拉取也失败：{e}"))
}

/// 解析继承链：返回自子向祖的节点列表
async fn resolve_chain(paths: &LauncherPaths, root_id: &str) -> Result<Vec<ChainNode>, String> {
    let mut chain: Vec<ChainNode> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut current = root_id.to_string();
    loop {
        if seen.contains(&current) {
            return Err(format!("版本继承链存在循环：{} → {current}", seen.join(" → ")));
        }
        if chain.len() >= MAX_CHAIN_DEPTH {
            return Err(format!("版本继承链超过最大深度 {MAX_CHAIN_DEPTH}"));
        }
        seen.push(current.clone());
        let json = fetch_local_first(paths, &current).await?;
        let parent = json
            .get("inheritsFrom")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        chain.push(ChainNode { id: current, json });
        match parent {
            Some(p) => current = p,
            None => break,
        }
    }
    Ok(chain)
}

/// 从整条链的 libraries / mainClass 检测加载器类型与版本
fn detect_loader(chain: &[ChainNode]) -> LoaderInfo {
    let mut loader = LoaderInfo {
        loader_type: "vanilla".into(),
        version: None,
        optifine_version: None,
        installed: true,
    };
    for node in chain {
        let libs = node
            .json
            .get("libraries")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for lib in &libs {
            let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
            // Gradle 坐标 group:artifact:version[:classifier]，取 artifact 与 version 段
            let segs: Vec<&str> = name.split(':').collect();
            if segs.len() < 3 {
                continue;
            }
            let (artifact, version) = (segs[1], segs[2]);
            match artifact {
                "fmlloader" | "forge" => {
                    // forge 坐标形如 net.minecraftforge:forge:1.20.1-47.2.0
                    loader.loader_type = "forge".into();
                    loader.version = Some(version.to_string());
                }
                "neoforge" | "neoforge-loader" => {
                    loader.loader_type = "neoforge".into();
                    loader.version = Some(version.to_string());
                }
                "fabric-loader" => {
                    loader.loader_type = "fabric".into();
                    loader.version = Some(version.to_string());
                }
                "quilt-loader" => {
                    loader.loader_type = "quilt".into();
                    loader.version = Some(version.to_string());
                }
                "OptiFine" => {
                    // optifine:OptiFine:1.20.1-HD_U_I6
                    loader.optifine_version = Some(version.to_string());
                }
                _ => {}
            }
        }
    }
    loader
}

/// 扫描并导入外部版本文件夹，返回本次导入数量（幂等：已有 .pie 元数据或
/// 失败标记的文件夹跳过）
pub async fn scan_and_import(paths: &LauncherPaths) -> usize {
    let mut imported = 0;
    let Ok(mut entries) = tokio::fs::read_dir(paths.versions_root()).await else {
        return 0;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let pie = dir.join(".pie");
        if pie.join("instance.json").is_file() || pie.join("import-failed").is_file() {
            continue; // 自家实例 / 已知失败
        }
        let Some(id) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        // 非版本文件夹（无 {id}.json）静默跳过，不产生任何痕迹
        if !dir.join(format!("{id}.json")).is_file() {
            continue;
        }
        match import_one(paths, &dir, &id).await {
            Ok(()) => imported += 1,
            Err(reason) => {
                // 失败标记：后续扫描跳过；用户修复后删标记重试
                if tokio::fs::create_dir_all(&pie).await.is_ok() {
                    let _ = tokio::fs::write(pie.join("import-failed"), &reason).await;
                }
                eprintln!("[foreign] 跳过外部版本 {id}：{reason}");
            }
        }
    }
    imported
}

/// 导入单个版本文件夹（写 .pie 元数据；不改动任何原有文件）
async fn import_one(paths: &LauncherPaths, dir: &Path, id: &str) -> Result<(), String> {
    // 有效性：{id}.json 存在且可解析（HMCL/PCL/官方版本文件夹的必要条件）
    let vjson_path = dir.join(format!("{id}.json"));
    let text = tokio::fs::read_to_string(&vjson_path)
        .await
        .map_err(|_| "缺少版本 json（非版本文件夹或文件缺失）".to_string())?;
    // 解析校验（内容经 chain 冻结使用，此处确保非法 json 提前失败并标记）
    let _: Value =
        serde_json::from_str(&text).map_err(|e| format!("版本 json 解析失败：{e}"))?;

    // 冻结继承链（无 inheritsFrom 时链仅含自身）
    let chain = resolve_chain(paths, id).await?;
    let merged = merge_chain(&chain)?;
    if merged.get("mainClass").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
        return Err("版本 json 缺少 mainClass（疑似损坏）".into());
    }

    // MC 版本：优先从继承链 libraries 推导——整合包根 json 的 id 常被自定义成
    // 整合包名（如 HMCL 装包生成 id="Booster FPS"），不能直接当版本号
    let root = chain.last().expect("链非空");
    let root_id = root
        .json
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or(id)
        .to_string();
    let mc_version = derive_mc_version_from_libraries(&merged)
        .or_else(|| {
            if looks_like_mc_version(&root_id) {
                Some(root_id.clone())
            } else {
                None
            }
        })
        .unwrap_or(root_id);
    let loader = detect_loader(&chain);

    // 写元数据（新增文件，零侵入）
    let meta = InstanceMeta {
        id: id.to_string(),
        name: id.to_string(),
        mc_version,
        icon: None,
        created_at: now_epoch(),
        last_launched_at: None,
        java_path: None,
        isolation: true,
        jvm: JvmSettings::default(),
    };
    tokio::fs::create_dir_all(dir.join(".pie"))
        .await
        .map_err(|e| format!("创建 .pie 目录失败：{e}"))?;
    tokio::fs::write(
        dir.join(".pie").join("instance.json"),
        serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| format!("写入 instance.json 失败：{e}"))?;
    tokio::fs::write(
        dir.join(".pie").join("loader.json"),
        serde_json::to_string_pretty(&loader).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| format!("写入 loader.json 失败：{e}"))?;
    tokio::fs::write(
        dir.join(".pie").join("version.json"),
        serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| format!("写入冻结 version.json 失败：{e}"))?;
    Ok(())
}

/// 折叠继承链（自祖先向子合并），返回冻结结果
fn merge_chain(chain: &[ChainNode]) -> Result<Value, String> {
    let mut iter = chain.iter().rev();
    let mut acc = iter.next().expect("链非空").json.clone();
    for node in iter {
        acc = merge_version_json(&acc, &node.json);
    }
    if let Some(obj) = acc.as_object_mut() {
        obj.remove("inheritsFrom");
        // id 改写为实例文件夹名（与 jar / ${version_name} 一致）
        obj.insert("id".to_string(), serde_json::json!(chain[0].id));
    }
    Ok(acc)
}

/// MC 版本形态校验：形如 1.21.1 / 1.8.9（多段全数字）。
/// 用于识别"版本号被误写成整合包名"的元数据（市场 facet 非法会 500/400）
pub(crate) fn looks_like_mc_version(s: &str) -> bool {
    let s = s.trim();
    if !s.contains('.') {
        return false;
    }
    s.split('.').all(|seg| {
        !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit())
    })
}

/// 从版本 json 的 libraries 推导真实 MC 版本：
///   Forge：net.minecraftforge:forge:1.20.1-47.2.0 / fmlloader:1.20.1-47.2.0
///   Fabric：net.fabricmc:intermediary:1.21.1
///   Quilt：org.quiltmc:hashed:1.21.1
///   OptiFine：optifine:OptiFine:1.20.1-HD_U_I6
///   NeoForge 1.20.2+：net.neoforged:neoforge:20.4.237（20.4 ↔ 1.20.4）
/// 整合包版本 json 的根 id 常被自定义成包名，libraries 是唯一可靠信号。
fn derive_mc_version_from_libraries(merged: &Value) -> Option<String> {
    let libs = merged.get("libraries")?.as_array()?;
    for lib in libs {
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let segs: Vec<&str> = name.split(':').collect();
        if segs.len() < 3 {
            continue;
        }
        let (artifact, version) = (segs[1], segs[2]);
        let leading = || {
            version
                .split(['-', '+'])
                .next()
                .unwrap_or(version)
                .to_string()
        };
        match artifact {
            "forge" | "fmlloader" | "minecraftforge" => {
                let mc = leading();
                if looks_like_mc_version(&mc) {
                    return Some(mc);
                }
            }
            "intermediary" | "hashed" if looks_like_mc_version(version) => {
                return Some(version.to_string());
            }
            "OptiFine" => {
                let mc = leading();
                if looks_like_mc_version(&mc) {
                    return Some(mc);
                }
            }
            "neoforge" | "neoforge-loader" => {
                let nums: Vec<u32> =
                    version.split('.').filter_map(|x| x.parse().ok()).collect();
                if nums.len() >= 2 && nums[0] >= 20 {
                    return Some(format!("1.{}.{}", nums[0], nums[1]));
                }
            }
            _ => {}
        }
    }
    None
}

/// 修复历史导入的外部实例的 mc_version 误记：旧版导入直接把继承链根 id（常为
/// 整合包名）当 MC 版本存入元数据，导致市场搜索 facet 非法被服务端拒绝。
/// 元数据值不像 MC 版本且冻结 version.json 存在时，从 libraries 重新推导并回写。
/// 返回修复数（幂等：值合法即跳过）。
pub async fn repair_mc_versions(paths: &LauncherPaths) -> usize {
    let mut fixed = 0;
    let Ok(mut entries) = tokio::fs::read_dir(paths.versions_root()).await else {
        return 0;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let dir = entry.path();
        let pie = dir.join(".pie");
        let meta_path = pie.join("instance.json");
        if !meta_path.is_file() {
            continue; // 非自家实例
        }
        let Ok(text) = tokio::fs::read_to_string(&meta_path).await else {
            continue;
        };
        let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(mc) = meta
            .get("mcVersion")
            .and_then(|v| v.as_str())
            .map(str::to_string)
        else {
            continue;
        };
        if looks_like_mc_version(&mc) {
            continue; // 已是合法版本形态
        }
        let Ok(vtext) = tokio::fs::read_to_string(pie.join("version.json")).await else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&vtext) else {
            continue;
        };
        if let (Some(new_mc), Some(obj)) =
            (derive_mc_version_from_libraries(&v), meta.as_object_mut())
        {
            obj.insert("mcVersion".into(), serde_json::json!(new_mc));
            if tokio::fs::write(
                &meta_path,
                serde_json::to_string_pretty(&meta).unwrap_or_default(),
            )
            .await
            .is_ok()
            {
                fixed += 1;
                eprintln!("[foreign] 修复 mc_version：{mc} → {new_mc}（{}）", dir.display());
            }
        }
    }
    fixed
}

/// 外部改动自愈：版本文件夹被用户在资源管理器中手动改名时，.pie\instance.json
/// 里的 id 仍是旧名，导致列表返回失效 id、所有操作报「实例不存在」。
/// 以文件夹名为准回写元数据，并对齐内部文件命名：
///   {旧id}.jar / {旧id}.json → {新id}.jar / {新id}.json
/// version json 与 .pie\version.json 的 id 字段同步改写。
/// 返回修复的实例数（幂等：一致即跳过）。
pub async fn reconcile_renamed_instances(paths: &LauncherPaths) -> usize {
    let mut fixed = 0;
    let Ok(mut entries) = tokio::fs::read_dir(paths.versions_root()).await else {
        return 0;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let meta_path = dir.join(".pie").join("instance.json");
        let Ok(text) = tokio::fs::read_to_string(&meta_path).await else {
            continue;
        };
        let Ok(mut meta) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(folder) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let Some(old_id) = meta.get("id").and_then(|v| v.as_str()).map(str::to_string) else {
            continue;
        };
        if old_id == folder {
            continue;
        }
        if let Err(e) = rename_instance_files(&dir, &old_id, &folder).await {
            eprintln!("[foreign] 实例目录改名对齐失败 {old_id} → {folder}：{e}");
            continue;
        }
        // 元数据回写（id 与文件夹名一致；name 若用户未自定义过则一并跟随）
        let name_follows = meta.get("name").and_then(|v| v.as_str()) == Some(old_id.as_str());
        if let Some(obj) = meta.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!(folder));
            if name_follows {
                obj.insert("name".to_string(), serde_json::json!(folder));
            }
        }
        if let Err(e) = tokio::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap_or_default()).await {
            eprintln!("[foreign] 实例元数据回写失败 {folder}：{e}");
            continue;
        }
        eprintln!("[foreign] 实例目录改名已对齐：{old_id} → {folder}");
        fixed += 1;
    }
    fixed
}

/// 文件夹改名后的内部文件对齐：主 jar、版本 json、冻结版 version.json 的 id 字段
async fn rename_instance_files(dir: &Path, old_id: &str, new_id: &str) -> Result<(), String> {
    // 主 jar：{旧id}.jar → {新id}.jar（新名已存在则跳过，不覆盖）
    let old_jar = dir.join(format!("{old_id}.jar"));
    let new_jar = dir.join(format!("{new_id}.jar"));
    if old_jar.is_file() && !new_jar.exists() {
        tokio::fs::rename(&old_jar, &new_jar)
            .await
            .map_err(|e| format!("重命名主 jar 失败：{e}"))?;
    }
    // 版本 json：改名并改写 id 字段
    let old_json = dir.join(format!("{old_id}.json"));
    let new_json = dir.join(format!("{new_id}.json"));
    if old_json.is_file() {
        let text = tokio::fs::read_to_string(&old_json)
            .await
            .map_err(|e| format!("读取版本 json 失败：{e}"))?;
        let mut vjson: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("版本 json 解析失败：{e}"))?;
        if let Some(obj) = vjson.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!(new_id));
        }
        tokio::fs::write(&new_json, serde_json::to_string_pretty(&vjson).unwrap_or_default())
            .await
            .map_err(|e| format!("写入版本 json 失败：{e}"))?;
        if old_json != new_json {
            let _ = tokio::fs::remove_file(&old_json).await;
        }
    }
    // 冻结版（外部实例导入产物）：仅改写 id 字段
    let frozen = dir.join(".pie").join("version.json");
    if frozen.is_file() {
        if let Ok(text) = tokio::fs::read_to_string(&frozen).await {
            if let Ok(mut vjson) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(obj) = vjson.as_object_mut() {
                    obj.insert("id".to_string(), serde_json::json!(new_id));
                }
                let _ = tokio::fs::write(&frozen, serde_json::to_string_pretty(&vjson).unwrap_or_default()).await;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("piemcl-foreign-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".minecraft/versions")).unwrap();
        dir
    }

    fn paths_of(root: &Path) -> LauncherPaths {
        LauncherPaths::new(root.join("PieMCL"), root.join(".minecraft"))
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[tokio::test]
    async fn reconciles_externally_renamed_folder() {
        let root = temp_root();
        let paths = paths_of(&root);
        let game = root.join(".minecraft");

        // 自家实例，用户在资源管理器里把文件夹从 old 改成 new-id
        let dir = game.join("versions/new-id");
        std::fs::create_dir_all(dir.join(".pie")).unwrap();
        std::fs::write(
            dir.join(".pie/instance.json"),
            r#"{ "id": "old", "name": "old", "mcVersion": "1.20.1", "createdAt": 1 }"#,
        )
        .unwrap();
        std::fs::write(dir.join("old.jar"), b"jar").unwrap();
        std::fs::write(
            dir.join("old.json"),
            r#"{ "id": "old", "mainClass": "net.minecraft.client.main.Main" }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join(".pie/version.json"),
            r#"{ "id": "old", "mainClass": "net.minecraft.client.main.Main" }"#,
        )
        .unwrap();

        let n = reconcile_renamed_instances(&paths).await;
        assert_eq!(n, 1);

        // 元数据以文件夹名为准
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(".pie/instance.json")).unwrap())
                .unwrap();
        assert_eq!(meta["id"], "new-id");
        assert_eq!(meta["name"], "new-id");
        // 内部文件对齐改名
        assert!(dir.join("new-id.jar").is_file());
        assert!(!dir.join("old.jar").exists());
        assert!(dir.join("new-id.json").is_file());
        assert!(!dir.join("old.json").exists());
        let vjson: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("new-id.json")).unwrap())
                .unwrap();
        assert_eq!(vjson["id"], "new-id");
        let frozen: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(".pie/version.json")).unwrap())
                .unwrap();
        assert_eq!(frozen["id"], "new-id");

        // 幂等：再次执行不再改动
        assert_eq!(reconcile_renamed_instances(&paths).await, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn imports_pcl_style_fabric_instance() {
        let root = temp_root();
        let paths = paths_of(&root);
        let game = root.join(".minecraft");

        // PCL 风格：原版父版本 + Fabric 子版本（继承链）
        write(
            &game.join("versions/1.20.1/1.20.1.json"),
            r#"{
                "id": "1.20.1", "mainClass": "net.minecraft.client.main.Main",
                "libraries": [ { "name": "com.mojang:brigadier:1.2.9" } ]
            }"#,
        );
        write(
            &game.join("versions/1.20.1-fabric/1.20.1-fabric.json"),
            r#"{
                "id": "1.20.1-fabric", "inheritsFrom": "1.20.1",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "libraries": [
                    { "name": "net.fabricmc:fabric-loader:0.16.9" },
                    { "name": "net.fabricmc:fabric-api:0.1.0" }
                ]
            }"#,
        );
        write(&game.join("versions/1.20.1-fabric/1.20.1-fabric.jar"), "jar");

        let n = scan_and_import(&paths).await;
        // Fabric 实例 + 其原版父版本（1.20.1 本身也是合法版本文件夹）都被导入
        assert_eq!(n, 2);

        let inst = game.join("versions/1.20.1-fabric");
        // 元数据就位
        assert!(inst.join(".pie/instance.json").is_file());
        // 加载器检测：fabric 0.16.9，installed=true
        let loader: LoaderInfo = serde_json::from_str(
            &std::fs::read_to_string(inst.join(".pie/loader.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(loader.loader_type, "fabric");
        assert_eq!(loader.version.as_deref(), Some("0.16.9"));
        assert!(loader.installed);
        // instance.json：MC 版本取继承链根 id
        let meta: InstanceMeta = serde_json::from_str(
            &std::fs::read_to_string(inst.join(".pie/instance.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta.mc_version, "1.20.1");
        assert!(meta.isolation);
        // 冻结版：继承链已合并（含父 libraries），id 改写为文件夹名，无 inheritsFrom
        let frozen: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(inst.join(".pie/version.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(frozen["id"], "1.20.1-fabric");
        assert!(frozen.get("inheritsFrom").is_none());
        assert_eq!(frozen["mainClass"], "net.fabricmc.loader.impl.launch.knot.KnotClient");
        let libs = frozen["libraries"].as_array().unwrap();
        assert!(libs.iter().any(|l| l["name"] == "com.mojang:brigadier:1.2.9"));
        // 原文件零侵入：json / jar 原样存在
        assert!(inst.join("1.20.1-fabric.json").is_file());
        assert!(inst.join("1.20.1-fabric.jar").is_file());
        assert!(game.join("versions/1.20.1/1.20.1.json").is_file());

        // 幂等：第二次扫描不再导入任何内容
        let n2 = scan_and_import(&paths).await;
        assert_eq!(n2, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn imports_vanilla_and_marks_broken() {
        let root = temp_root();
        let paths = paths_of(&root);
        let game = root.join(".minecraft");

        // 官方启动器风格：纯原版版本文件夹
        write(
            &game.join("versions/1.21.4/1.21.4.json"),
            r#"{ "id": "1.21.4", "mainClass": "net.minecraft.client.main.Main" }"#,
        );
        write(&game.join("versions/1.21.4/1.21.4.jar"), "jar");
        // 损坏：json 存在但非法
        write(&game.join("versions/broken/broken.json"), "not-json{");
        // 非版本文件夹：无 json
        std::fs::create_dir_all(game.join("versions/random_dir")).unwrap();

        let n = scan_and_import(&paths).await;
        assert_eq!(n, 1); // 只有 vanilla 导入
        assert!(game.join("versions/1.21.4/.pie/instance.json").is_file());
        assert!(game.join("versions/broken/.pie/import-failed").is_file());
        assert!(!game.join("versions/random_dir/.pie").exists(), "非版本文件夹不应生成 .pie");

        // 幂等 + 失败标记短路
        assert_eq!(scan_and_import(&paths).await, 0);
        assert!(!game.join("versions/broken/.pie/instance.json").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn detects_forge_and_optifine() {
        let root = temp_root();
        let paths = paths_of(&root);
        let game = root.join(".minecraft");
        write(
            &game.join("versions/forge/forge.json"),
            r#"{
                "id": "forge", "mainClass": "cpw.mods.modlauncher.Launcher",
                "libraries": [
                    { "name": "net.minecraftforge:forge:1.20.1-47.2.0" },
                    { "name": "optifine:OptiFine:1.20.1-HD_U_I6" }
                ]
            }"#,
        );
        let n = scan_and_import(&paths).await;
        assert_eq!(n, 1);
        let loader: LoaderInfo = serde_json::from_str(
            &std::fs::read_to_string(game.join("versions/forge/.pie/loader.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(loader.loader_type, "forge");
        assert_eq!(loader.version.as_deref(), Some("1.20.1-47.2.0"));
        assert_eq!(loader.optifine_version.as_deref(), Some("1.20.1-HD_U_I6"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
