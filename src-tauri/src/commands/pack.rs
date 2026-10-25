// 整合包导入 / 导出命令
//
// pick_modpack：Rust 侧文件选择对话框 → 解析预览（PackPreview）
// import_modpack：创建实例 → 安装加载器 → 下载包内文件 → 解压 overrides →
//                 补全游戏资源；全程发 pack-import-progress 事件，
//                 下载进度复用 download-progress（kind = "pack"）
// export_modpack：保存对话框 → 收集 Modrinth 来源模组（files 清单）与本地内容
//                 （overrides）→ 生成 .mrpack
//
// 支持：Modrinth .mrpack 与 CurseForge zip（CF 下载需 API Key）；
// 加载器覆盖 fabric / quilt（profile 合并）与 forge / neoforge（installer 管线）。

use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, State};

use super::instance::create_instance_core;
use super::AppState;
use crate::download::model::{DownloadRequest, HashAlgo};
use crate::instance::model::LoaderInfo;
use crate::mods::{curseforge as cf, pack};
use crate::mods::store::ModSources;
use crate::version::resolve::merge_version_json;

fn emit_progress(
    app: &AppHandle,
    stage: &str,
    message: &str,
    current: Option<u64>,
    total: Option<u64>,
) {
    let _ = app.emit(
        "pack-import-progress",
        serde_json::json!({ "stage": stage, "message": message, "current": current, "total": total }),
    );
}

/// 实例内简单读取（与 game.rs 的 read_version_json 行为一致）
async fn read_version_json(state: &AppState, instance_id: &str) -> Result<Value, String> {
    let path = state.paths.instance_version_json(instance_id);
    let text = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| format!("实例 {instance_id} 缺少版本元数据（version.json）"))?;
    serde_json::from_str(&text).map_err(|e| format!("version.json 解析失败：{e}"))
}

/// 串行下载一组请求（kind="pack"，实例级互斥 + oneshot 等待终态）
async fn download_requests(
    state: &AppState,
    app: &AppHandle,
    instance_id: &str,
    requests: Vec<DownloadRequest>,
) -> Result<(), String> {
    if requests.is_empty() {
        return Ok(());
    }
    let lock_key = instance_id.to_string();
    let task_id = format!("pack-{}", uuid::Uuid::new_v4().simple());
    {
        let mut locks = state.active_downloads.lock().unwrap();
        if locks.contains_key(&lock_key) {
            return Err("该实例已有下载任务在进行中".into());
        }
        locks.insert(lock_key.clone(), task_id.clone());
    }

    let opts = crate::download::TaskOptions {
        task_id: task_id.clone(),
        kind: "pack".into(),
        instance_id: Some(instance_id.to_string()),
    };
    super::register_retry(
        &state.retry_defs,
        &task_id,
        opts.clone(),
        requests.clone(),
        Some(lock_key.clone()),
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let tx = std::sync::Mutex::new(Some(tx));
    let cb = super::make_retry_cb(
        state.retry_defs.clone(),
        state.active_downloads.clone(),
        app.clone(),
        task_id.clone(),
        Some(tx),
    );

    let start = state.download.start_task(opts, requests, cb).await;
    if let Err(e) = start {
        state.active_downloads.lock().unwrap().remove(&lock_key);
        super::remove_retry(&state.retry_defs, &task_id);
        return Err(e);
    }
    rx.await
        .map_err(|_| "整合包下载任务意外结束".to_string())?
        .map_err(String::from)
}

/// 补全实例的 MC 基础资源（client.jar / libraries / assets，与启动前检查同逻辑）
async fn ensure_instance_resources(
    state: &AppState,
    app: &AppHandle,
    instance_id: &str,
) -> Result<(), String> {
    let version = read_version_json(state, instance_id).await?;
    let game_root = state.paths.game_root().to_path_buf();
    let settings = state.settings.read().await;
    let (mirrors, official_first) = crate::commands::game::source_policy(&settings).await;

    let index_id = version
        .pointer("/assetIndex/id")
        .and_then(|v| v.as_str())
        .unwrap_or("legacy")
        .to_string();
    let index_path = game_root.join("assets").join("indexes").join(format!("{index_id}.json"));
    if !index_path.exists() {
        crate::game::planner::ensure_asset_index(&version, &index_path, &mirrors, official_first)
            .await
            .map_err(String::from)?;
    }
    let mut requests =
        crate::game::planner::plan_base(&version, &game_root, &mirrors, official_first).map_err(String::from)?;
    requests.extend(
        crate::game::planner::plan_objects(&index_path, &game_root, &mirrors, official_first)
            .await
            .map_err(String::from)?,
    );
    // 引擎内置"存在 + 哈希匹配即跳过"的增量判定，直接全量交给引擎
    download_requests(state, app, instance_id, requests).await
}

/// 预览信息（pick_modpack）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPreview {
    pub kind: String,
    pub name: String,
    pub mc_version: String,
    pub loader_type: String,
    pub loader_version: Option<String>,
    pub mod_count: usize,
    /// 整合包文件路径（导入时回传）
    pub path: String,
}

/// 弹出文件选择对话框并解析整合包预览；用户取消返回 None
#[tauri::command]
pub async fn pick_modpack(app: AppHandle) -> Result<Option<PackPreview>, String> {
    use tauri_plugin_dialog::DialogExt;

    let app2 = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .add_filter("模组整合包", &["mrpack", "zip"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("对话框任务执行失败：{e}"))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| format!("获取所选路径失败：{e}"))?;
    parse_pack_file(path.to_string_lossy().into_owned())
        .await
        .map(Some)
}

/// 解析指定路径的整合包预览（拖入文件导入用）
#[tauri::command]
pub async fn parse_pack_file(path: String) -> Result<PackPreview, String> {
    // 扩展名预检：非整合包格式给出明确错误
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if ext != "mrpack" && ext != "zip" {
        let shown = if ext.is_empty() { "（无扩展名）" } else { &ext };
        return Err(format!(
            "不支持的整合包格式：{shown}，仅支持 .mrpack（Modrinth）与 .zip（CurseForge）"
        ));
    }
    let parse_path = path.clone();
    let parsed = tauri::async_runtime::spawn_blocking(move || {
        pack::parse_pack(std::path::Path::new(&parse_path))
    })
    .await
    .map_err(|e| format!("解析任务执行失败：{e}"))??;
    Ok(match parsed {
        pack::ParsedPack::Modrinth { preview, .. } => PackPreview {
            kind: preview.kind,
            name: preview.name,
            mc_version: preview.mc_version,
            loader_type: preview.loader_type,
            loader_version: preview.loader_version,
            mod_count: preview.mod_count,
            path: path.clone(),
        },
        pack::ParsedPack::CurseForge { preview, .. } => PackPreview {
            kind: preview.kind,
            name: preview.name,
            mc_version: preview.mc_version,
            loader_type: preview.loader_type,
            loader_version: preview.loader_version,
            mod_count: preview.mod_count,
            path,
        },
    })
}

/// 导出内容清单条目（PCL2 风格勾选树）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportItem {
    /// 勾选键（导出时回传；文件项为相对 minecraft/ 的路径）
    pub key: String,
    pub label: String,
    pub hint: Option<String>,
    pub default_checked: bool,
}

/// 导出内容清单分组
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportGroup {
    pub key: String,
    pub label: String,
    /// 组主体恒选（游戏本体 / Mod），勾选树仅控制附属内容
    pub required: bool,
    pub items: Vec<ExportItem>,
}

/// 列出目录下的文件（相对 rel 前缀的键 + 文件名）
fn list_dir_files(mc_dir: &Path, rel: &str) -> Vec<ExportItem> {
    let mut out = Vec::new();
    let root = mc_dir.join(rel);
    let Ok(read) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in read.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        out.push(ExportItem {
            key: format!("{rel}/{name}"),
            label: name,
            hint: None,
            default_checked: true,
        });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

/// 目录是否存在且包含内容
fn dir_has_content(mc_dir: &Path, rel: &str) -> bool {
    let root = mc_dir.join(rel);
    let Ok(mut read) = std::fs::read_dir(&root) else {
        return false;
    };
    read.next().is_some()
}

/// 扫描实例 minecraft/ 目录，生成可导出的内容清单（按 PCL2 风格分组）
#[tauri::command]
pub async fn list_export_items(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<ExportGroup>, String> {
    let mc_dir = state.instances.game_dir(&instance_id).await.map_err(String::from)?;
    if !mc_dir.is_dir() {
        return Err("实例目录不存在".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut groups: Vec<ExportGroup> = Vec::new();

        // 游戏本体（恒选）
        let mut game_items: Vec<ExportItem> = Vec::new();
        if mc_dir.join("options.txt").is_file() {
            game_items.push(ExportItem {
                key: "options".into(),
                label: "游戏本体设置".into(),
                hint: Some("键位、音量、视频设置等".into()),
                default_checked: true,
            });
        }
        let personal_exists =
            mc_dir.join("command_history.txt").is_file() || mc_dir.join("hotbar.nbc").is_file();
        if personal_exists {
            game_items.push(ExportItem {
                key: "personal".into(),
                label: "游戏本体个人信息".into(),
                hint: Some("命令历史、已保存的快捷栏".into()),
                default_checked: false,
            });
        }
        groups.push(ExportGroup {
            key: "game".into(),
            label: "游戏本体".into(),
            required: true,
            items: game_items,
        });

        // Mod（恒选，本体即 mods/ 与下载清单）
        let mut mod_items: Vec<ExportItem> = Vec::new();
        if dir_has_content(&mc_dir, "config") {
            mod_items.push(ExportItem {
                key: "config".into(),
                label: "Mod 设置".into(),
                hint: None,
                default_checked: true,
            });
        }
        if dir_has_content(&mc_dir, "config/jei") {
            mod_items.push(ExportItem {
                key: "jei".into(),
                label: "JEI 个人信息".into(),
                hint: Some("物品收藏夹等".into()),
                default_checked: false,
            });
        }
        groups.push(ExportGroup {
            key: "mods".into(),
            label: "Mod".into(),
            required: true,
            items: mod_items,
        });

        // 资源包 / 光影包（逐文件勾选）
        let rp = list_dir_files(&mc_dir, "resourcepacks");
        if !rp.is_empty() {
            groups.push(ExportGroup {
                key: "resourcepacks".into(),
                label: "资源包".into(),
                required: false,
                items: rp,
            });
        }
        let sp = list_dir_files(&mc_dir, "shaderpacks");
        if !sp.is_empty() {
            groups.push(ExportGroup {
                key: "shaders".into(),
                label: "光影包".into(),
                required: false,
                items: sp,
            });
        }

        // 截图（整组一项）
        if dir_has_content(&mc_dir, "screenshots") {
            let count = std::fs::read_dir(mc_dir.join("screenshots"))
                .map(|r| r.flatten().count())
                .unwrap_or(0);
            groups.push(ExportGroup {
                key: "screenshots".into(),
                label: "截图".into(),
                required: false,
                items: vec![ExportItem {
                    key: "screenshots".into(),
                    label: format!("截图（{count} 张）"),
                    hint: None,
                    default_checked: false,
                }],
            });
        }

        // 服务器列表
        if mc_dir.join("servers.dat").is_file() {
            groups.push(ExportGroup {
                key: "servers".into(),
                label: "多人游戏服务器列表".into(),
                required: false,
                items: vec![ExportItem {
                    key: "servers".into(),
                    label: "servers.dat".into(),
                    hint: None,
                    default_checked: true,
                }],
            });
        }

        groups
    })
    .await
    .map_err(|e| format!("扫描任务执行失败：{e}"))
}

/// 递归收集目录下所有文件进 overrides（rel 为相对 minecraft/ 的前缀）
fn collect_subtree(
    mc_dir: &Path,
    dir: &Path,
    rel: &str,
    out: &mut Vec<(String, PathBuf)>,
) {
    let Ok(mut read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        let child_rel = format!("{rel}/{name}");
        let child = e.path();
        if meta.is_dir() {
            collect_subtree(mc_dir, &child, &child_rel, out);
        } else {
            out.push((child_rel, child));
        }
    }
}

/// 追加单个文件进 overrides（存在才加入）
fn push_override_file(mc_dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let abs = mc_dir.join(rel);
    if abs.is_file() {
        out.push((rel.to_string(), abs));
    }
}

/// 追加一层目录内容进 overrides（子目录递归）
fn push_override_dir(mc_dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let root = mc_dir.join(rel);
    let Ok(mut read) = std::fs::read_dir(&root) else { return };
    for e in read.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        let child_rel = format!("{rel}/{name}");
        if meta.is_dir() {
            let sub_root = mc_dir.join(&child_rel);
            collect_subtree(mc_dir, &sub_root, &child_rel, out);
        } else {
            let abs = mc_dir.join(&child_rel);
            out.push((child_rel, abs));
        }
    }
}

/// 按导出勾选清单收集附属内容（rel 相对 minecraft/，进 overrides 同名路径）
fn collect_selected_overrides(mc_dir: &Path, selected: &[String], out: &mut Vec<(String, PathBuf)>) {
    let has = |k: &str| selected.iter().any(|s| s == k);
    if has("options") {
        push_override_file(mc_dir, "options.txt", out);
    }
    if has("personal") {
        push_override_file(mc_dir, "command_history.txt", out);
        push_override_file(mc_dir, "hotbar.nbc", out);
    }
    if has("config") {
        push_override_dir(mc_dir, "config", out);
    } else if has("jei") {
        push_override_dir(mc_dir, "config/jei", out);
    }
    // 资源包 / 光影包：勾选键即相对路径
    for s in selected {
        if s.starts_with("resourcepacks/") || s.starts_with("shaderpacks/") {
            push_override_file(mc_dir, s, out);
        }
    }
    if has("screenshots") {
        push_override_dir(mc_dir, "screenshots", out);
    }
    if has("servers") {
        push_override_file(mc_dir, "servers.dat", out);
    }
}

/// 导入整合包：创建实例 → 安装加载器 → 下载文件 → 解压 overrides → 补全资源
/// 返回新实例 ID（进度经 pack-import-progress / download-progress 事件推送）
#[tauri::command]
pub async fn import_modpack(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> Result<String, String> {
    let pack_path = path.clone();
    let parsed = tauri::async_runtime::spawn_blocking(move || pack::parse_pack(std::path::Path::new(&pack_path)))
        .await
        .map_err(|e| format!("解析任务执行失败：{e}"))??;

    let (preview, mrpack_files, cf_files) = match &parsed {
        pack::ParsedPack::Modrinth { preview, files } => (preview.clone(), files.clone(), None),
        pack::ParsedPack::CurseForge { preview, files } => (preview.clone(), Vec::new(), Some(files.clone())),
    };
    if preview.mc_version.is_empty() {
        return Err("整合包缺少 Minecraft 版本信息".into());
    }

    // 1. 创建实例
    emit_progress(&app, "creating", &format!("正在创建实例「{name}」…"), None, None);
    let loader = LoaderInfo {
        loader_type: preview.loader_type.clone(),
        version: preview.loader_version.clone(),
        optifine_version: None,
        installed: false,
    };
    let summary =
        create_instance_core(&app, &state, &name, &preview.mc_version, loader).await?;
    let instance_id = summary.id.clone();
    let game_root = state.paths.game_root().to_path_buf();
    // 整合包文件（mods / overrides）落游戏工作目录（新建实例默认隔离 = 版本文件夹）
    let game_dir = state.paths.instance_game_dir(&instance_id, true);

    // 2. 安装加载器（fabric/quilt：profile 合并；forge/neoforge：installer 管线）
    if matches!(preview.loader_type.as_str(), "forge" | "neoforge") {
        emit_progress(
            &app,
            "loader",
            "正在安装 Forge 加载器（含补丁处理器，需要几分钟）…",
            None,
            None,
        );
        let lv = preview
            .loader_version
            .clone()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| "整合包缺少加载器版本信息".to_string())?;
        let version_json = read_version_json(&state, &instance_id).await?;
        let settings = state.settings.read().await;
        let (mirrors, official_first) =
            crate::commands::game::source_policy(&settings).await;
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
            task_id,
            instance_id: instance_id.clone(),
            paths: state.paths.clone(),
            game_root: game_root.clone(),
            version_json,
            loader_type: preview.loader_type.clone(),
            full_version: lv,
            mc_version: preview.mc_version.clone(),
            mirrors,
            official_first,
        };
        let result = crate::loader::installer::install_forge_like(ctx).await;
        state.active_downloads.lock().unwrap().remove(&instance_id);
        result?;
    } else if preview.loader_type != "vanilla" {
        emit_progress(&app, "loader", "正在安装加载器…", None, None);
        let lv = preview
            .loader_version
            .clone()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| "整合包缺少加载器版本信息".to_string())?;
        let profile = state
            .versions
            .fetch_loader_profile(&preview.loader_type, &preview.mc_version, &lv)
            .await
            .map_err(String::from)?;
        let base = read_version_json(&state, &instance_id).await?;
        let mut merged = merge_version_json(&base, &profile);
        if let Some(obj) = merged.as_object_mut() {
            obj.remove("inheritsFrom");
            // id = 实例名（版本文件夹名），与 jar / ${version_name} 保持一致
            obj.insert("id".to_string(), serde_json::json!(instance_id));
        }
        let s = state.settings.read().await;
        let official_first = s.download_source != "mirror";
        let mirrors = if s.download_source == "official" {
            Vec::new()
        } else {
            s.mirrors.clone()
        };
        drop(s);
        let requests =
            crate::game::planner::plan_base(&profile, &game_root, &mirrors, official_first).map_err(String::from)?;
        download_requests(&state, &app, &instance_id, requests).await?;

        // 写回 version.json 与 loader.json
        let loader_info = LoaderInfo {
            loader_type: preview.loader_type.clone(),
            version: Some(lv),
            optifine_version: None,
            installed: true,
        };
        let v_text = serde_json::to_string_pretty(&merged)
            .map_err(|e| format!("序列化 version.json 失败：{e}"))?;
        let l_text = serde_json::to_string_pretty(&serde_json::to_value(&loader_info).unwrap_or_default())
            .map_err(|e| format!("序列化 loader.json 失败：{e}"))?;
        tokio::fs::write(state.paths.instance_version_json(&instance_id), v_text)
            .await
            .map_err(|e| format!("写入 version.json 失败：{e}"))?;
        tokio::fs::write(
            state.paths.instance_meta_dir(&instance_id).join("loader.json"),
            l_text,
        )
        .await
        .map_err(|e| format!("写入 loader.json 失败：{e}"))?;
    }

    // 3. 下载包内文件（mods / resourcepacks 等）
    let mut requests: Vec<DownloadRequest> = Vec::new();
    match &mrpack_files[..] {
        [] => {}
        _ => {
            for f in &mrpack_files {
                let mut req = DownloadRequest::new(
                    f.path.clone(),
                    crate::download::mirror::mcimirror_candidates(&f.urls),
                    game_dir.join(&f.path),
                );
                if let Some(sha) = &f.sha1 {
                    req.expected = Some((HashAlgo::Sha1, sha.clone()));
                }
                req.size = f.size;
                requests.push(req);
            }
        }
    }
    if let Some(cf_files) = &cf_files {
        emit_progress(
            &app,
            "resolving",
            "正在解析 CurseForge 文件下载链接…",
            None,
            None,
        );
        let key = state.settings.read().await.curseforge_api_key.clone();
        if key.trim().is_empty() {
            return Err(
                "导入 CurseForge 整合包需要 API Key，请在「设置 → 模组管理」中填入".into(),
            );
        }
        for (pid, fid) in cf_files {
            let f = cf::get_file(&key, *pid, *fid).await.map_err(String::from)?;
            if !f.compatible_with(&preview.mc_version) {
                return Err(format!(
                    "整合包含有不兼容当前 MC 版本的文件：{}（支持 {}）",
                    f.display_name,
                    f.game_versions.join(", ")
                ));
            }
            let spec_url = f.download_url.clone().ok_or_else(|| {
                format!(
                    "「{}」的作者禁用了第三方下载，无法自动导入该文件",
                    f.display_name
                )
            })?;
            let rel = format!("mods/{}", f.file_name);
            let mut req = DownloadRequest::new(
                rel.clone(),
                crate::download::mirror::mcimirror_candidates(std::slice::from_ref(&spec_url)),
                game_dir.join(&rel),
            );
            if let Some(sha) = f.sha1() {
                req.expected = Some((HashAlgo::Sha1, sha));
            }
            req.size = Some(f.file_size);
            requests.push(req);
        }
    }
    if !requests.is_empty() {
        emit_progress(
            &app,
            "downloading",
            &format!("正在下载 {} 个文件…", requests.len()),
            Some(0),
            Some(requests.len() as u64),
        );
        download_requests(&state, &app, &instance_id, requests).await?;
    }

    // 4. 解压 overrides
    emit_progress(&app, "extracting", "正在释放整合包配置文件…", None, None);
    let p2 = path.clone();
    let mcd2 = game_dir.clone();
    tauri::async_runtime::spawn_blocking(move || pack::extract_overrides(std::path::Path::new(&p2), &mcd2))
        .await
        .map_err(|e| format!("解压任务执行失败：{e}"))??;

    // 5. 补全游戏资源（client / libraries / assets）
    emit_progress(&app, "resources", "正在补全游戏资源…", None, None);
    ensure_instance_resources(&state, &app, &instance_id).await?;

    emit_progress(&app, "done", "导入完成", None, None);
    let _ = app.emit("instances-changed", ());
    Ok(instance_id)
}

/// 导出当前实例为 .mrpack（保存对话框；返回保存路径）
#[tauri::command]
pub async fn export_modpack(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    selected: Vec<String>,
) -> Result<String, String> {
    use tauri_plugin_dialog::DialogExt;

    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    let default_name = format!("{}.mrpack", detail.summary.name);

    // 保存对话框（阻塞 API 放 spawn_blocking）
    let app2 = app.clone();
    let default_name2 = default_name.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .add_filter("Modrinth 整合包", &["mrpack"])
            .set_file_name(&default_name2)
            .blocking_save_file()
    })
    .await
    .map_err(|e| format!("对话框任务执行失败：{e}"))?;
    let Some(picked) = picked else {
        return Err("已取消导出".into());
    };
    let dest = picked
        .into_path()
        .map_err(|e| format!("获取保存路径失败：{e}"))?;

    let inst = state.paths.instance_dir(&instance_id);
    let mc_dir = state.instances.game_dir(&instance_id).await.map_err(String::from)?;

    // 收集导出内容（阻塞 I/O 放 spawn_blocking）：Mod 恒选，附属内容按勾选清单收集
    let sources = ModSources::load(&inst);
    let mc_dir2 = mc_dir.clone();
    let (hosted, overrides) = tauri::async_runtime::spawn_blocking(move || {
        let mut hosted: Vec<(String, PathBuf, Option<String>, Option<String>, u64)> = Vec::new();
        let mut overrides: Vec<(String, PathBuf)> = Vec::new();
        let has = |k: &str| selected.iter().any(|s| s == k);

        // Mod 本体：Modrinth 来源进 files 清单，本地来源进 overrides
        let mods_path = mc_dir2.join("mods");
        if let Ok(read) = std::fs::read_dir(&mods_path) {
            for e in read.flatten() {
                let Ok(meta) = e.metadata() else { continue };
                if !meta.is_file() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".jar") {
                    continue; // 仅导出启用中的模组
                }
                let path = e.path();
                match sources.get(crate::mods::ResourceType::Mod, &name) {
                    Some(s) if s.origin == "modrinth" && s.download_url.is_some() => {
                        let sha1 = s
                            .sha1
                            .clone()
                            .or_else(|| pack::sha1_file(&path).ok());
                        hosted.push((
                            format!("mods/{name}"),
                            path,
                            sha1,
                            s.download_url.clone(),
                            meta.len(),
                        ));
                    }
                    _ => {
                        // 本地来源或无记录：进 overrides
                        overrides.push((format!("mods/{name}"), path));
                    }
                }
            }
        }

        // 附属内容按勾选收集（rel 相对 minecraft/，进 overrides 同名路径）
        collect_selected_overrides(&mc_dir2, &selected, &mut overrides);
        (hosted, overrides)
    })
    .await
    .map_err(|e| format!("收集任务执行失败：{e}"))?;

    let input = pack::ExportInput {
        instance_name: detail.summary.name.clone(),
        instance_id: instance_id.clone(),
        mc_version: detail.summary.mc_version.clone(),
        loader_type: detail.summary.loader_type.clone(),
        loader_version: detail.summary.loader_version.clone(),
        hosted_files: hosted,
        override_files: overrides,
    };
    let dest2 = dest.clone();
    tauri::async_runtime::spawn_blocking(move || pack::export_mrpack(&input, &dest2))
        .await
        .map_err(|e| format!("导出任务执行失败：{e}"))??;
    Ok(dest.to_string_lossy().into_owned())
}
