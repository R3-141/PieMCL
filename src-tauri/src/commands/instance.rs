// 实例相关 Tauri commands
//
// 前端约定：参数 camelCase；所有返回 Result<T, String>（中文错误文案）。
// 事件：
//   instance-create-progress  { stage, message }   创建进度
//   instances-changed         ()                   实例增删改后推送，前端重拉列表

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::instance::model::{
    InstanceDetail, InstanceSummary, LoaderInfo, UpdateInstancePatch,
};
use crate::version::resolve::{self, merge_version_json};
use super::AppState;

/// 支持的加载器类型（OptiFine 非加载器，仅作 Forge 附加项）
const LOADER_TYPES: &[&str] = &["vanilla", "forge", "fabric", "neoforge", "quilt"];

fn emit_changed(app: &AppHandle) {
    let _ = app.emit("instances-changed", ());
}

#[tauri::command]
pub async fn list_instances(
    state: State<'_, AppState>,
) -> Result<Vec<InstanceSummary>, String> {
    state.instances.list().await.map_err(String::from)
}

#[tauri::command]
pub async fn get_instance(
    state: State<'_, AppState>,
    id: String,
) -> Result<InstanceDetail, String> {
    state.instances.get(&id).await.map_err(String::from)
}

#[tauri::command]
pub async fn create_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    mc_version: String,
    loader_type: Option<String>,
    loader_version: Option<String>,
    optifine_version: Option<String>,
) -> Result<InstanceSummary, String> {
    let loader_type = loader_type.unwrap_or_else(|| "vanilla".to_string());
    if !LOADER_TYPES.contains(&loader_type.as_str()) {
        return Err(format!("不支持的加载器类型：{loader_type}"));
    }
    // vanilla 不带加载器版本
    let loader_version = if loader_type == "vanilla" {
        None
    } else {
        loader_version
    };
    // OptiFine 仅能搭配 Forge
    if optifine_version.is_some() && loader_type != "forge" {
        return Err("OptiFine 仅支持与 Forge 搭配使用".to_string());
    }
    let loader = LoaderInfo {
        loader_type,
        version: loader_version,
        optifine_version,
        installed: false,
    };
    create_instance_core(&app, &state, &name, &mc_version, loader).await
}

/// 创建实例的共享核心（普通创建与整合包导入共用）
pub(crate) async fn create_instance_core(
    app: &AppHandle,
    state: &AppState,
    name: &str,
    mc_version: &str,
    loader: LoaderInfo,
) -> Result<InstanceSummary, String> {
    let emit = |stage: &str, message: &str| {
        let _ = app.emit(
            "instance-create-progress",
            json!({ "stage": stage, "message": message }),
        );
    };

    emit("fetch_manifest", "正在获取版本清单…");
    let manifest = state
        .versions
        .fetch_manifest(false)
        .await
        .map_err(String::from)?;
    if !manifest.versions.iter().any(|v| v.id == mc_version) {
        return Err(format!("版本清单中找不到版本 {mc_version}"));
    }

    emit("fetch_version", "正在下载并解析版本元数据…");
    let resolved = resolve::fetch_version_resolved(&state.versions, mc_version)
        .await
        .map_err(String::from)?;

    emit("write", "正在写入实例文件…");
    let summary = state
        .instances
        .create(name, mc_version, &resolved, loader.clone())
        .await
        .map_err(String::from)?;

    // Fabric / Quilt：创建即完成加载器合并（version.json + loader.json installed=true），
    // loader 库随后随资源下载一并补全——用户创建后可直接下载/启动，无需感知"骨架"状态；
    // profile 拉取失败则降级为骨架，启动前检查会自动补装兜底
    if matches!(loader.loader_type.as_str(), "fabric" | "quilt") {
        if let Some(lv) = loader.version.clone().filter(|v| !v.trim().is_empty()) {
            emit("loader", "正在合并加载器元数据…");
            match state
                .versions
                .fetch_loader_profile(&loader.loader_type, mc_version, &lv)
                .await
            {
                Ok(profile) => {
                    let paths = &state.paths;
                    let base = tokio::fs::read_to_string(paths.instance_version_json(&summary.id))
                        .await
                        .map_err(|e| format!("读取 version.json 失败：{e}"))?;
                    let base_json: Value = serde_json::from_str(&base)
                        .map_err(|e| format!("version.json 解析失败：{e}"))?;
                    let mut merged = merge_version_json(&base_json, &profile);
                    if let Some(obj) = merged.as_object_mut() {
                        obj.remove("inheritsFrom");
                        // id = 实例名（版本文件夹名），与 jar / ${version_name} 保持一致
                        obj.insert("id".to_string(), serde_json::json!(summary.id));
                    }
                    let installed = LoaderInfo {
                        loader_type: loader.loader_type.clone(),
                        version: Some(lv),
                        optifine_version: loader.optifine_version.clone(),
                        installed: true,
                    };
                    let v_text = serde_json::to_string_pretty(&merged)
                        .map_err(|e| format!("序列化 version.json 失败：{e}"))?;
                    let l_text = serde_json::to_string_pretty(
                        &serde_json::to_value(&installed).unwrap_or_default(),
                    )
                    .map_err(|e| format!("序列化 loader.json 失败：{e}"))?;
                    tokio::fs::write(paths.instance_version_json(&summary.id), v_text)
                        .await
                        .map_err(|e| format!("写入 version.json 失败：{e}"))?;
                    tokio::fs::write(
                        paths.instance_meta_dir(&summary.id).join("loader.json"),
                        l_text,
                    )
                    .await
                    .map_err(|e| format!("写入 loader.json 失败：{e}"))?;
                }
                Err(e) => {
                    eprintln!("[create] 加载器 profile 拉取失败，降级为骨架（启动时自动补装）：{e}");
                }
            }
        }
    }

    emit("done", "创建完成");
    emit_changed(app);
    Ok(summary)
}

#[tauri::command]
pub async fn delete_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    permanent: bool,
) -> Result<(), String> {
    state
        .instances
        .delete(&id, permanent)
        .await
        .map_err(String::from)?;
    emit_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn duplicate_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    new_name: String,
    copy_saves: bool,
) -> Result<InstanceSummary, String> {
    let summary = state
        .instances
        .duplicate(&id, &new_name, copy_saves)
        .await
        .map_err(String::from)?;
    emit_changed(&app);
    Ok(summary)
}

#[tauri::command]
pub async fn update_instance_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: UpdateInstancePatch,
) -> Result<InstanceDetail, String> {
    let detail = state
        .instances
        .update_settings(&id, patch)
        .await
        .map_err(String::from)?;
    emit_changed(&app);
    Ok(detail)
}

#[tauri::command]
pub async fn open_instance_folder(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let dir = state.instances.instance_dir(&id);
    if !dir.is_dir() {
        return Err(format!("实例 {id} 不存在"));
    }
    opener::open(&dir).map_err(|e| format!("打开文件夹失败：{e}"))
}
