// 游戏资源与加载器安装命令
//
// install_game_resources：按实例冻结的 version.json 补全
//   client.jar / libraries / asset index / assets objects（增量：缺失或损坏才下载）
// install_loader：安装 Fabric / Quilt（拉取 profile JSON → 成功后合并冻结 version.json
//   并下载 loader libraries → 更新 loader.json）
//   Forge / NeoForge 需要 install_profile 解析与 processor 执行，留待后续阶段（见 TODO）
//
// 进度统一经 download-progress 事件推送（payload 含 taskId，前端凭它暂停/继续/取消），
// 前端零轮询。同一实例同时只允许一个下载任务（active_downloads 实例锁）。

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::core::error::{AppError, AppResult};
use crate::core::settings::AppSettings;
use crate::game::planner;
use crate::instance::model::LoaderInfo;
use crate::version::resolve::merge_version_json;
use super::start_task;

/// 读取实例冻结的 version.json
async fn read_version_json(state: &AppState, instance_id: &str) -> AppResult<Value> {
    let path = state.paths.instance_version_json(instance_id);
    let text = tokio::fs::read_to_string(&path).await.map_err(|_| {
        AppError::msg(format!("实例 {instance_id} 缺少版本元数据（version json）"))
    })?;
    Ok(serde_json::from_str(&text)?)
}

/// 按下载源策略生成下载候选参数（供资源补全 / 加载器安装 / 整合包导入共用）：
/// auto（官方直连优先，失败轮换镜像）→ 官方在前；official → 空镜像表（仅官方）；
/// mirror → 镜像在前（现状）。官方在前由 planner 的 official_first 标志控制。
pub(crate) async fn source_policy(settings: &AppSettings) -> (Vec<String>, bool) {
    match settings.download_source.as_str() {
        "official" => (Vec::new(), true),
        "mirror" => (settings.mirrors.clone(), false),
        _ => (settings.mirrors.clone(), true),
    }
}

/// 补全实例的游戏资源：client.jar + libraries + asset index + assets objects
/// 返回 task_id（进度见 download-progress 事件）
#[tauri::command]
pub async fn install_game_resources(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<String, String> {
    if !state.paths.instance_dir(&instance_id).is_dir() {
        return Err(format!("实例 {instance_id} 不存在"));
    }
    let version = read_version_json(&state, &instance_id).await.map_err(String::from)?;
    let game_root = state.paths.game_root().to_path_buf();
    let settings = state.settings.read().await;
    let (mirrors, official_first) = source_policy(&settings).await;

    // asset index 先就绪（objects 规划依赖其内容）
    let index_id = version
        .pointer("/assetIndex/id")
        .and_then(|v| v.as_str())
        .unwrap_or("legacy")
        .to_string();
    let index_path = game_root.join("assets").join("indexes").join(format!("{index_id}.json"));
    if !index_path.exists() {
        planner::ensure_asset_index(&version, &index_path, &mirrors, official_first)
            .await
            .map_err(String::from)?;
    }

    let mut requests = planner::plan_base(&version, &game_root, &mirrors, official_first)
        .map_err(String::from)?;
    let objects = planner::plan_objects(&index_path, &game_root, &mirrors, official_first)
        .await
        .map_err(String::from)?;
    requests.extend(objects);

    let task_id = format!("res-{}", uuid::Uuid::new_v4().simple());
    start_task(
        &state.download,
        state.active_downloads.clone(),
        state.retry_defs.clone(),
        &app,
        task_id.clone(),
        "resources",
        &instance_id,
        requests,
        None,
    )
    .await?;
    Ok(task_id)
}

/// 安装加载器（当前支持 Fabric / Quilt；Forge / NeoForge 留待后续阶段）
/// 返回 task_id（loader libraries 下载进度见 download-progress 事件）
#[tauri::command]
pub async fn install_loader(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    loader_type: Option<String>,
    loader_version: Option<String>,
) -> Result<String, String> {
    let dir = state.paths.instance_dir(&instance_id);
    if !dir.is_dir() {
        return Err(format!("实例 {instance_id} 不存在"));
    }
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    let mc_version = detail.summary.mc_version;

    // 加载器类型与版本：参数优先，否则沿用 loader.json
    let loader: LoaderInfo =
        match tokio::fs::read_to_string(state.paths.instance_meta_dir(&instance_id).join("loader.json"))
            .await
        {
            Ok(t) => serde_json::from_str(&t)
                .map_err(|e| format!("解析 loader.json 失败：{e}"))?,
            Err(_) => LoaderInfo::vanilla(),
        };
    let lt = loader_type.unwrap_or(loader.loader_type);
    if lt == "vanilla" {
        return Err("原版实例无需安装加载器".to_string());
    }
    let lv = match loader_version.or(loader.version) {
        Some(v) if !v.trim().is_empty() => v,
        _ => return Err("缺少加载器版本，请先指定".to_string()),
    };
    match lt.as_str() {
        "fabric" | "quilt" => {}
        "forge" | "neoforge" => {
            // ---- Forge / NeoForge：installer 管线（loader 模块）----
            // 前置校验 → 全程持实例锁 → 原版资源补全 → installer 下载 →
            // 解析/解压 → 组件库下载 → processors → 产物校验 → 写回
            let version_json = read_version_json(&state, &instance_id).await.map_err(String::from)?;
            let settings = state.settings.read().await;
            let (mirrors, official_first) = source_policy(&settings).await;
            drop(settings);

            let task_id = format!("loader-{}", uuid::Uuid::new_v4().simple());
            {
                let mut locks = state.active_downloads.lock().unwrap();
                if locks.contains_key(&instance_id) {
                    return Err("该实例已有下载任务在进行中".into());
                }
                locks.insert(instance_id.clone(), task_id.clone());
            }
            let ctx = crate::loader::installer::ForgeInstallCtx {
                app: Some(app.clone()),
                download: state.download.clone(),
                active: state.active_downloads.clone(),
                retry_defs: state.retry_defs.clone(),
                task_id: task_id.clone(),
                instance_id: instance_id.clone(),
                paths: state.paths.clone(),
                game_root: state.paths.game_root().to_path_buf(),
                version_json,
                loader_type: lt.clone(),
                full_version: lv.clone(),
                mc_version,
                mirrors,
                official_first,
            };
            let result = crate::loader::installer::install_forge_like(ctx).await;
            state.active_downloads.lock().unwrap().remove(&instance_id);
            return result.map(|_| task_id);
        }
        other => return Err(format!("不支持的加载器类型：{other}")),
    }
    // fabric / quilt：拉取 profile JSON（标准 version JSON，inheritsFrom 指向 MC）
    let profile = state
        .versions
        .fetch_loader_profile(&lt, &mc_version, &lv)
        .await
        .map_err(String::from)?;

    // 预合并：MC 冻结版 + profile（成功后写回 version.json，失败/取消不动原文件）
    let mut merged = merge_version_json(&read_version_json(&state, &instance_id).await?, &profile);
    if let Some(obj) = merged.as_object_mut() {
        obj.remove("inheritsFrom");
        // id = 实例名（版本文件夹名），与 jar / ${version_name} 保持一致
        obj.insert("id".to_string(), serde_json::json!(instance_id));
    }

    // 规划 loader 自身的 libraries（MC 侧资源由 install_game_resources 负责）
    let game_root = state.paths.game_root().to_path_buf();
    let settings = state.settings.read().await;
    let (mirrors, official_first) = source_policy(&settings).await;
    let requests = planner::plan_base(&profile, &game_root, &mirrors, official_first)
        .map_err(String::from)?;

    // 完成回调：写回 version.json（合并版）与 loader.json（installed=true），推送实例变更
    let lt2 = lt.clone();
    let lv2 = lv.clone();
    let merged2 = merged.clone();
    let iid2 = instance_id.clone();
    let on_completed: Box<dyn Fn(AppHandle) + Send + Sync> = Box::new(move |app| {
        let lt = lt2.clone();
        let lv = lv2.clone();
        let merged = merged2.clone();
        let iid = iid2.clone();
        tauri::async_runtime::spawn(async move {
            let paths = app.state::<AppState>().paths.clone();
            let info = LoaderInfo {
                loader_type: lt,
                version: Some(lv),
                optifine_version: None,
                installed: true,
            };
            if let (Ok(v), Ok(l)) = (
                serde_json::to_string_pretty(&merged),
                serde_json::to_string_pretty(&serde_json::to_value(&info).unwrap_or_default()),
            ) {
                let _ = tokio::fs::write(paths.instance_version_json(&iid), v).await;
                let _ =
                    tokio::fs::write(paths.instance_meta_dir(&iid).join("loader.json"), l).await;
            }
            let _ = app.emit("instances-changed", ());
        });
    });

    let task_id = format!("loader-{}", uuid::Uuid::new_v4().simple());
    start_task(
        &state.download,
        state.active_downloads.clone(),
        state.retry_defs.clone(),
        &app,
        task_id.clone(),
        "loader",
        &instance_id,
        requests,
        Some(on_completed),
    )
    .await?;
    Ok(task_id)
}
