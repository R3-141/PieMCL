// 实例存储层：目录结构管理与 instance.json / loader.json 读写
//
// 实例 = 游戏根 versions\{实例名}\ 下的一个版本文件夹（与主流启动器一致）：
//   {实例名}.jar / {实例名}.json     主 jar 与冻结合并版 version.json（id = 实例名）
//   .pie\instance.json               实例元数据
//   .pie\loader.json                 加载器骨架信息
//   .pie\logs\                       启动器捕获的日志
// 实例 id 即文件夹名（= 实例名清洗去重后的结果），重命名实例 = 重命名文件夹。
// 所有 I/O 走 tokio 异步；删除默认回收站，permanent 时物理删除；
// 复制跳过日志与崩溃报告，saves 可选。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::error::{AppError, AppResult};
use crate::core::paths::{unique_folder_name, LauncherPaths};
use super::model::{
    InstanceDetail, InstanceMeta, InstanceSummary, JvmSettings, LoaderInfo, UpdateInstancePatch,
};

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 读取实例元数据与加载器信息（.pie\ 下）
async fn read_instance_files(dir: &Path) -> AppResult<(InstanceMeta, LoaderInfo)> {
    let meta_text = tokio::fs::read_to_string(dir.join(".pie").join("instance.json")).await?;
    let meta: InstanceMeta = serde_json::from_str(&meta_text)?;
    let loader = match tokio::fs::read_to_string(dir.join(".pie").join("loader.json")).await {
        Ok(t) => serde_json::from_str(&t)?,
        Err(_) => LoaderInfo::vanilla(),
    };
    Ok((meta, loader))
}

fn to_summary(meta: &InstanceMeta, loader: &LoaderInfo) -> InstanceSummary {
    InstanceSummary {
        id: meta.id.clone(),
        name: meta.name.clone(),
        icon: meta.icon.clone(),
        mc_version: meta.mc_version.clone(),
        loader_type: loader.loader_type.clone(),
        loader_version: loader.version.clone(),
        optifine_version: loader.optifine_version.clone(),
        last_launched_at: meta.last_launched_at,
    }
}

fn to_detail(meta: &InstanceMeta, loader: &LoaderInfo) -> InstanceDetail {
    InstanceDetail {
        summary: to_summary(meta, loader),
        created_at: meta.created_at,
        java_path: meta.java_path.clone(),
        isolation: meta.isolation,
        jvm: meta.jvm.clone(),
    }
}

/// 实例存储
#[derive(Clone)]
pub struct InstanceStore {
    paths: LauncherPaths,
}

impl InstanceStore {
    pub fn new(paths: LauncherPaths) -> Self {
        Self { paths }
    }

    /// 创建实例版本文件夹并写入元数据（version json 为已解析冻结结果，
    /// id 字段改写为实例文件夹名以保持 jar / json / ${version_name} 三者一致）
    pub async fn create(
        &self,
        name: &str,
        mc_version: &str,
        resolved_version_json: &serde_json::Value,
        loader: LoaderInfo,
    ) -> AppResult<InstanceSummary> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::msg("实例名称不能为空"));
        }
        let id = unique_folder_name(&self.paths.versions_root(), name);
        let dir = self.paths.instance_dir(&id);

        tokio::fs::create_dir_all(dir.join(".pie")).await?;

        let meta = InstanceMeta {
            id: id.clone(),
            name: name.to_string(),
            mc_version: mc_version.to_string(),
            icon: None,
            created_at: now_epoch(),
            last_launched_at: None,
            java_path: None,
            isolation: true,
            jvm: JvmSettings::default(),
        };

        let write_meta_json = async |file: &str, value: &serde_json::Value| -> AppResult<()> {
            let text = serde_json::to_string_pretty(value)?;
            tokio::fs::write(dir.join(".pie").join(file), text).await?;
            Ok(())
        };
        write_meta_json("instance.json", &serde_json::to_value(&meta)?).await?;
        write_meta_json("loader.json", &serde_json::to_value(&loader)?).await?;

        // 冻结 version.json → {id}.json，id 改写为实例名（与版本文件夹 / jar 一致）
        let mut version_json = resolved_version_json.clone();
        if let Some(obj) = version_json.as_object_mut() {
            obj.insert("id".to_string(), serde_json::json!(id));
        }
        tokio::fs::write(
            self.paths.instance_version_json(&id),
            serde_json::to_string_pretty(&version_json)?,
        )
        .await?;

        Ok(to_summary(&meta, &loader))
    }

    /// 列出所有实例摘要（先自动导入外部版本文件夹——HMCL/PCL/官方启动器兼容，
    /// 幂等且零侵入；随后扫描 versions\ 下含 .pie\instance.json 的文件夹，
    /// 跳过损坏目录，避免拖垮列表）
    pub async fn list(&self) -> AppResult<Vec<InstanceSummary>> {
        // 开箱即用：自动识别其他启动器留下的版本文件夹（失败标记短路，开销可忽略）
        let _ = super::foreign::scan_and_import(&self.paths).await;
        // 外部改动自愈：文件夹被手动改名时回写元数据与内部文件命名
        let _ = super::foreign::reconcile_renamed_instances(&self.paths).await;
        // 修复历史导入的 mc_version 误记（整合包名被当成 MC 版本，市场请求非法）
        let _ = super::foreign::repair_mc_versions(&self.paths).await;
        let mut out = Vec::new();
        let mut entries = match tokio::fs::read_dir(self.paths.versions_root()).await {
            Ok(e) => e,
            Err(_) => return Ok(out),
        };
        while let Some(entry) = entries.next_entry().await? {
            let dir = entry.path();
            if !dir.is_dir() || !dir.join(".pie").join("instance.json").is_file() {
                continue;
            }
            if let Ok((meta, loader)) = read_instance_files(&dir).await {
                out.push(to_summary(&meta, &loader));
            }
        }
        out.sort_by(|a, b| b.last_launched_at.cmp(&a.last_launched_at));
        Ok(out)
    }

    /// 读取单个实例详情
    pub async fn get(&self, id: &str) -> AppResult<InstanceDetail> {
        let dir = self.paths.instance_dir(id);
        if !dir.is_dir() {
            return Err(AppError::msg(format!("实例 {id} 不存在")));
        }
        let (meta, loader) = read_instance_files(&dir).await?;
        Ok(to_detail(&meta, &loader))
    }

    /// 实例的游戏工作目录（按 isolation 派生；供各 command 快捷取用）
    pub async fn game_dir(&self, id: &str) -> AppResult<PathBuf> {
        let meta = self.read_meta(id).await?;
        Ok(self.paths.instance_game_dir(id, meta.isolation))
    }

    /// 读取实例元数据（轻量，供只需要 isolation 等单字段的调用方）
    async fn read_meta(&self, id: &str) -> AppResult<InstanceMeta> {
        let dir = self.paths.instance_dir(id);
        let text = tokio::fs::read_to_string(dir.join(".pie").join("instance.json"))
            .await
            .map_err(|_| AppError::msg(format!("实例 {id} 不存在或元数据损坏")))?;
        serde_json::from_str(&text).map_err(|e| AppError::msg(format!("instance.json 解析失败：{e}")))
    }

    /// 删除实例：默认移入回收站，permanent 时物理删除
    pub async fn delete(&self, id: &str, permanent: bool) -> AppResult<()> {
        let dir = self.paths.instance_dir(id);
        if !dir.is_dir() {
            return Err(AppError::msg(format!("实例 {id} 不存在")));
        }
        if permanent {
            tokio::fs::remove_dir_all(&dir).await?;
        } else {
            // trash 为同步 API，放阻塞线程执行
            let d = dir.clone();
            tokio::task::spawn_blocking(move || trash::delete(d))
                .await
                .map_err(|e| AppError::msg(format!("执行回收站操作失败：{e}")))??;
        }
        Ok(())
    }

    /// 复制实例：跳过日志 / 崩溃报告 / natives，saves 按参数。
    /// 复制后按新名改写文件夹名、{id}.jar / {id}.json 文件名与 json id。
    pub async fn duplicate(
        &self,
        id: &str,
        new_name: &str,
        copy_saves: bool,
    ) -> AppResult<InstanceSummary> {
        let src = self.paths.instance_dir(id);
        if !src.is_dir() {
            return Err(AppError::msg(format!("实例 {id} 不存在")));
        }
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(AppError::msg("新实例名称不能为空"));
        }
        let new_id = unique_folder_name(&self.paths.versions_root(), new_name);
        let dst = self.paths.instance_dir(&new_id);

        copy_dir_filtered(&src, &dst, copy_saves).await?;

        // 主 jar / 版本 json 按新名改名（目标是副本目录内的旧名文件）
        for ext in ["jar", "json"] {
            let old_file = dst.join(format!("{id}.{ext}"));
            if old_file.is_file() {
                tokio::fs::rename(&old_file, dst.join(format!("{new_id}.{ext}")))
                    .await
                    .map_err(|e| AppError::msg(format!("改名 {id}.{ext} 失败：{e}")))?;
            }
        }

        // 改写元数据：新 id / 名称 / 时间戳
        let (mut meta, loader) = read_instance_files(&dst).await?;
        meta.id = new_id.clone();
        meta.name = new_name.to_string();
        meta.created_at = now_epoch();
        meta.last_launched_at = None;
        let text = serde_json::to_string_pretty(&meta)?;
        tokio::fs::write(dst.join(".pie").join("instance.json"), text).await?;

        // 版本 json 的 id 字段同步改写
        let vjson_path = dst.join(format!("{new_id}.json"));
        if let Ok(text) = tokio::fs::read_to_string(&vjson_path).await {
                    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("id".to_string(), serde_json::json!(new_id));
                        }
                        if let Ok(t) = serde_json::to_string_pretty(&v) {
                            let _ = tokio::fs::write(&vjson_path, t).await;
                        }
                    }
                }

        Ok(to_summary(&meta, &loader))
    }

    /// 更新实例设置并返回最新详情。
    /// 名称变更 = 重命名版本文件夹 + {id}.jar / {id}.json 改名 + json id 改写，
    /// 返回的详情携带新 id（前端需以返回值刷新选中态）。
    pub async fn update_settings(
        &self,
        id: &str,
        patch: UpdateInstancePatch,
    ) -> AppResult<InstanceDetail> {
        let dir = self.paths.instance_dir(id);
        if !dir.is_dir() {
            return Err(AppError::msg(format!("实例 {id} 不存在")));
        }
        let (mut meta, loader) = read_instance_files(&dir).await?;

        // 名称变更 → 版本文件夹整体改名（jar / json / 元数据 id 同步）
        if let Some(name) = &patch.name {
            let name = name.trim();
            if name.is_empty() {
                return Err(AppError::msg("实例名称不能为空"));
            }
            if name != meta.name {
                let new_id = {
                    // 占位新目录名避免与自身冲突
                    let tmp_marker = dir.join(".pie").join(".renaming");
                    tokio::fs::write(&tmp_marker, b"").await?;
                    let nid = unique_folder_name(&self.paths.versions_root(), name);
                    let _ = tokio::fs::remove_file(&tmp_marker).await;
                    nid
                };
                let new_dir = self.paths.instance_dir(&new_id);
                tokio::fs::rename(&dir, &new_dir)
                    .await
                    .map_err(|e| AppError::msg(format!("重命名实例文件夹失败：{e}")))?;
                for ext in ["jar", "json"] {
                    let old_file = new_dir.join(format!("{id}.{ext}"));
                    if old_file.is_file() {
                        tokio::fs::rename(&old_file, new_dir.join(format!("{new_id}.{ext}")))
                            .await
                            .map_err(|e| AppError::msg(format!("改名 {id}.{ext} 失败：{e}")))?;
                    }
                }
                // 版本 json id 同步改写
                let vjson_path = new_dir.join(format!("{new_id}.json"));
                if let Ok(text) = tokio::fs::read_to_string(&vjson_path).await {
                    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(obj) = v.as_object_mut() {
                            obj.insert("id".to_string(), serde_json::json!(new_id));
                        }
                        if let Ok(t) = serde_json::to_string_pretty(&v) {
                            let _ = tokio::fs::write(&vjson_path, t).await;
                        }
                    }
                }
                meta.id = new_id;
                meta.name = name.to_string();
            }
        }
        if let Some(v) = patch.java_path {
            meta.java_path = tri_state_string(v, "Java 路径")?;
        }
        if let Some(min) = patch.min_memory_mb {
            meta.jvm.min_memory_mb = min;
        }
        if let Some(max) = patch.max_memory_mb {
            if !(1024..=32768).contains(&max) {
                return Err(AppError::msg("最大内存需在 1024–32768 MB 之间"));
            }
            meta.jvm.max_memory_mb = max;
        }
        if let Some(auto) = patch.auto_memory {
            meta.jvm.auto_memory = auto;
        }
        if let Some(args) = patch.extra_args {
            meta.jvm.extra_args = args;
        }
        if let Some(preset) = patch.preset {
            if !matches!(preset.as_str(), "default" | "low_spec" | "big_modpack") {
                return Err(AppError::msg(format!("未知 JVM 预设：{preset}")));
            }
            meta.jvm.preset = preset;
        }
        if let Some(fs) = patch.fullscreen {
            meta.jvm.fullscreen = fs;
        }
        if let Some(w) = patch.window_width {
            meta.jvm.window_width = tri_state_u32(w, "窗口宽度")?;
        }
        if let Some(h) = patch.window_height {
            meta.jvm.window_height = tri_state_u32(h, "窗口高度")?;
        }
        if let Some(iso) = patch.isolation {
            meta.isolation = iso;
        }
        if meta.jvm.min_memory_mb > meta.jvm.max_memory_mb && meta.jvm.min_memory_mb > 0 {
            return Err(AppError::msg("最小内存不能大于最大内存"));
        }

        let new_dir = self.paths.instance_dir(&meta.id);
        let text = serde_json::to_string_pretty(&meta)?;
        tokio::fs::write(new_dir.join(".pie").join("instance.json"), text).await?;
        Ok(to_detail(&meta, &loader))
    }

    pub fn instance_dir(&self, id: &str) -> PathBuf {
        self.paths.instance_dir(id)
    }

    /// 记录最后启动时间（游戏进程成功拉起后调用；失败静默，不影响启动流程）
    pub async fn touch_launched(&self, id: &str) {
        let dir = self.paths.instance_dir(id);
        let Ok((mut meta, _)) = read_instance_files(&dir).await else {
            return;
        };
        meta.last_launched_at = Some(now_epoch());
        if let Ok(text) = serde_json::to_string_pretty(&meta) {
            let _ = tokio::fs::write(dir.join(".pie").join("instance.json"), text).await;
        }
    }
}

/// 三态字符串字段：Null → None（恢复默认）；String → 去空白后非空为 Some
fn tri_state_string(v: serde_json::Value, label: &str) -> AppResult<Option<String>> {
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => {
            let t = s.trim().to_string();
            Ok(if t.is_empty() { None } else { Some(t) })
        }
        _ => Err(AppError::msg(format!("{label}格式不正确"))),
    }
}

/// 三态数字字段：Null → None（恢复默认）；正整数 → Some
fn tri_state_u32(v: serde_json::Value, label: &str) -> AppResult<Option<u32>> {
    match v {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => n
            .as_u64()
            .map(|x| Some(x as u32))
            .ok_or_else(|| AppError::msg(format!("{label}格式不正确"))),
        _ => Err(AppError::msg(format!("{label}格式不正确"))),
    }
}

/// 复制时按相对路径跳过指定目录
fn should_skip(rel: &Path, copy_saves: bool) -> bool {
    let comps: Vec<&str> = rel
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    match comps.as_slice() {
        [".pie", "logs"] => true,      // 启动器日志不随实例复制
        [".pie", "trash"] => true,     // 回收站不随实例复制
        ["crash-reports"] => true,
        ["natives"] => true,           // natives 每次启动重新解压
        ["saves"] => !copy_saves,
        _ => false,
    }
}

/// 递归复制目录（迭代实现，跳过 should_skip 命中的目录）
async fn copy_dir_filtered(src: &Path, dst: &Path, copy_saves: bool) -> AppResult<()> {
    let mut stack: Vec<(PathBuf, PathBuf)> = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        tokio::fs::create_dir_all(&to).await?;
        let mut entries = tokio::fs::read_dir(&from).await?;
        while let Some(entry) = entries.next_entry().await? {
            let child_from = entry.path();
            let child_to = to.join(entry.file_name());
            let rel = child_from.strip_prefix(src).unwrap_or(&child_from).to_path_buf();
            if entry.file_type().await?.is_dir() {
                if !should_skip(&rel, copy_saves) {
                    stack.push((child_from, child_to));
                }
            } else {
                tokio::fs::copy(&child_from, &child_to).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::model::LoaderInfo;

    fn test_paths() -> LauncherPaths {
        let root = std::env::temp_dir().join(format!("piemcl-store-{}", uuid::Uuid::new_v4()));
        let game = root.join(".minecraft");
        LauncherPaths::new(root, game)
    }

    #[tokio::test]
    async fn create_uses_name_as_folder_and_rewrites_id() {
        let paths = test_paths();
        paths.ensure_dirs().await.unwrap();
        let store = InstanceStore::new(paths.clone());
        let resolved = serde_json::json!({ "id": "1.20.1", "mainClass": "Main" });
        let s = store
            .create("  我的世界/服:务器 ", "1.20.1", &resolved, LoaderInfo::vanilla())
            .await
            .unwrap();
        // 非法字符被清洗，文件夹名 = 清洗后的实例名
        assert_eq!(s.id, "我的世界服务器");
        assert_eq!(s.name, "我的世界/服:务器");
        let dir = paths.instance_dir(&s.id);
        assert!(dir.join(".pie/instance.json").is_file());
        assert!(dir.join(format!("{}.json", s.id)).is_file());
        // version json id 已改写为文件夹名
        let v: serde_json::Value = serde_json::from_str(
            &tokio::fs::read_to_string(paths.instance_version_json(&s.id)).await.unwrap(),
        )
        .unwrap();
        assert_eq!(v["id"], s.id);
        let _ = std::fs::remove_dir_all(paths.root().parent().unwrap());
    }

    #[tokio::test]
    async fn rename_moves_folder_and_files() {
        let paths = test_paths();
        paths.ensure_dirs().await.unwrap();
        let store = InstanceStore::new(paths.clone());
        let resolved = serde_json::json!({ "id": "1.20.1" });
        let s = store.create("旧名", "1.20.1", &resolved, LoaderInfo::vanilla()).await.unwrap();
        std::fs::write(paths.instance_dir("旧名").join("旧名.jar"), b"j").unwrap();

        let patch = UpdateInstancePatch {
            name: Some("新名".into()),
            ..Default::default()
        };
        let detail = store.update_settings(&s.id, patch).await.unwrap();
        assert_eq!(detail.summary.id, "新名");
        assert!(!paths.instance_dir("旧名").exists());
        assert!(paths.instance_dir("新名").join("新名.jar").is_file());
        assert!(paths.instance_dir("新名").join("新名.json").is_file());
        assert!(paths.instance_dir("新名").join(".pie/instance.json").is_file());
        let _ = std::fs::remove_dir_all(paths.root().parent().unwrap());
    }

    #[tokio::test]
    async fn duplicate_renames_jar_and_json() {
        let paths = test_paths();
        paths.ensure_dirs().await.unwrap();
        let store = InstanceStore::new(paths.clone());
        let resolved = serde_json::json!({ "id": "1.20.1" });
        let s = store.create("原版", "1.20.1", &resolved, LoaderInfo::vanilla()).await.unwrap();
        std::fs::write(paths.instance_dir("原版").join("原版.jar"), b"j").unwrap();

        let dup = store.duplicate(&s.id, "原版副本", false).await.unwrap();
        assert_eq!(dup.id, "原版副本");
        assert!(paths.instance_dir("原版副本").join("原版副本.jar").is_file());
        assert!(!paths.instance_dir("原版副本").join("原版.jar").exists());
        let v: serde_json::Value = serde_json::from_str(
            &tokio::fs::read_to_string(paths.instance_version_json("原版副本")).await.unwrap(),
        )
        .unwrap();
        assert_eq!(v["id"], "原版副本");
        let _ = std::fs::remove_dir_all(paths.root().parent().unwrap());
    }
}
