// 模组管理命令
//
// 所有操作都作用于明确的 instance_id（参数强制传入），Rust 侧不持有
// “当前实例”状态，避免跨实例污染。列表变更后发 mods-changed 事件，
// 前端据此刷新当前实例的模组列表。
//
// 阻塞说明：jar 解压读取与目录扫描放 spawn_blocking，不卡命令线程。

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};

use super::AppState;
use crate::download::model::{DownloadRequest, HashAlgo};
use crate::download::ProgressCallback;
use crate::mods::{curseforge as cf, local, ResourceType};
use crate::mods::modrinth::{self, DepRef, InstallPlan, OnlineVersion, SearchHit, VersionFile};
use crate::mods::store::{ModSource, ModSources};

use std::collections::HashSet;

/// Fabric API 的 Modrinth 项目 ID（附加组件安装去重用）
const FABRIC_API_PID: &str = "P7dR8mSH";

/// 统一安装目标格式：`mr:<version_id>` / `cf:<mod_id>:<file_id>`
fn parse_target(s: &str) -> Result<(&str, Option<(&str, &str)>), String> {
    if let Some(id) = s.strip_prefix("mr:") {
        return Ok(("modrinth", Some((id, ""))));
    }
    if let Some(rest) = s.strip_prefix("cf:") {
        let (m, f) = rest
            .split_once(':')
            .ok_or_else(|| format!("非法的 CurseForge 安装目标：{s}"))?;
        return Ok(("curseforge", Some((m, f))));
    }
    Err(format!("未知的安装目标格式：{s}"))
}

/// 下载规格（平台统一中间形态）
struct DlSpec {
    origin: String,
    project_id: String,
    version_id: String,
    version_number: String,
    file: VersionFile,
}

fn cf_file_to_spec(f: &cf::CfFile) -> Result<DlSpec, String> {
    let url = f.download_url.clone().ok_or_else(|| {
        format!(
            "「{}」的作者禁用了第三方下载，请到 CurseForge 官网手动下载",
            f.display_name
        )
    })?;
    Ok(DlSpec {
        origin: "curseforge".into(),
        project_id: f.mod_id.to_string(),
        version_id: f.id.to_string(),
        version_number: f.display_name.clone(),
        file: VersionFile {
            url,
            filename: f.file_name.clone(),
            primary: true,
            size: f.file_size,
            sha1: f.sha1(),
        },
    })
}

/// CF 文件 → OnlineVersion（版本列表展示用）
fn cf_file_to_version(f: &cf::CfFile, loader: &str) -> OnlineVersion {
    OnlineVersion {
        id: f.id.to_string(),
        project_id: f.mod_id.to_string(),
        version_number: f.display_name.clone(),
        version_type: "release".into(),
        game_versions: f.game_versions.clone(),
        loaders: vec![loader.into()],
        dependencies: cf::required_dep_ids(f)
            .into_iter()
            .map(|id| DepRef {
                version_id: None,
                project_id: Some(id.to_string()),
                dependency_type: "required".into(),
            })
            .collect(),
        files: vec![VersionFile {
            url: f.download_url.clone().unwrap_or_default(),
            filename: f.file_name.clone(),
            primary: true,
            size: f.file_size,
            sha1: f.sha1(),
        }],
    }
}

/// CF 搜索结果条目 → SearchHit
fn cf_hit(m: &serde_json::Value) -> Option<SearchHit> {
    Some(SearchHit {
        project_id: m.get("id")?.as_i64()?.to_string(),
        slug: m
            .get("slug")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .into(),
        title: m
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .into(),
        description: m
            .get("summary")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .into(),
        downloads: m.get("downloadCount").and_then(|x| x.as_u64()).unwrap_or(0),
        icon_url: m
            .pointer("/logo/thumbnailUrl")
            .or_else(|| m.pointer("/logo/url"))
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        author: m
            .pointer("/authors/0/name")
            .and_then(|x| x.as_str())
            .map(String::from),
        origin: "curseforge".into(),
        display_categories: m
            .get("categories")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        c.get("name")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_ascii_lowercase())
                    })
                    .collect()
            })
            .unwrap_or_default(),
        date_modified: m
            .get("dateModified")
            .and_then(|x| x.as_str())
            .map(String::from),
    })
}

/// 拖放 / 从文件安装的结果（三类分开返回，前端分别提示）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalInstallResult {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<String>,
}

fn emit_changed(app: &AppHandle, instance_id: &str) {
    let _ = app.emit("mods-changed", serde_json::json!({ "instanceId": instance_id }));
}

/// 列出实例社区资源（模组 / 数据包 / 光影 / 资源包，按 resource_type 定目录）
#[tauri::command]
pub async fn list_mods(
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
) -> Result<Vec<local::ModInfo>, String> {
    let kind = ResourceType::parse(&resource_type)?;
    let inst = state.paths.instance_dir(&instance_id);
    let mc = state.instances.game_dir(&instance_id).await.map_err(String::from)?;
    let retention = state.settings.read().await.trash_retention_days;
    tauri::async_runtime::spawn_blocking(move || local::scan_mods(&inst, &mc, retention, kind))
        .await
        .map_err(|e| format!("扫描任务执行失败：{e}"))
}

/// 启用 / 禁用（模组专属；同目录重命名 .jar ↔ .jar.disabled）
#[tauri::command]
pub async fn toggle_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    file_name: String,
    enable: bool,
) -> Result<(), String> {
    let kind = ResourceType::parse(&resource_type)?;
    let mc = state.instances.game_dir(&instance_id).await.map_err(String::from)?;
    tauri::async_runtime::spawn_blocking(move || local::toggle_mod(&mc, kind, &file_name, enable))
        .await
        .map_err(|e| format!("任务执行失败：{e}"))?
        .map_err(String::from)?;
    emit_changed(&app, &instance_id);
    Ok(())
}

/// 删除资源 → 实例 .trash/（retention_days=0 时直接永久删除）
#[tauri::command]
pub async fn delete_mods(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    file_names: Vec<String>,
) -> Result<Vec<String>, String> {
    let kind = ResourceType::parse(&resource_type)?;
    let inst = state.paths.instance_dir(&instance_id);
    let mc = state.instances.game_dir(&instance_id).await.map_err(String::from)?;
    // 回收站入口仅模组页提供；数据包 / 光影 / 资源包删除即永久删除
    let retention = if kind.is_mod() {
        state.settings.read().await.trash_retention_days
    } else {
        0
    };
    let removed = tauri::async_runtime::spawn_blocking(move || {
        local::delete_mods(&inst, &mc, kind, &file_names, retention)
    })
    .await
    .map_err(|e| format!("任务执行失败：{e}"))?
    .map_err(String::from)?;
    emit_changed(&app, &instance_id);
    Ok(removed)
}

/// 回收站列表
#[tauri::command]
pub async fn list_trash(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<local::TrashItem>, String> {
    let inst = state.paths.instance_dir(&instance_id);
    tauri::async_runtime::spawn_blocking(move || local::list_trash(&inst))
        .await
        .map_err(|e| format!("任务执行失败：{e}"))
}

/// 从回收站恢复到类型对应目录
#[tauri::command]
pub async fn restore_mod(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    trash_name: String,
) -> Result<(), String> {
    let kind = ResourceType::parse(&resource_type)?;
    let inst = state.paths.instance_dir(&instance_id);
    let mc = state.instances.game_dir(&instance_id).await.map_err(String::from)?;
    tauri::async_runtime::spawn_blocking(move || local::restore_mod(&inst, &mc, kind, &trash_name))
        .await
        .map_err(|e| format!("任务执行失败：{e}"))?
        .map_err(String::from)?;
    emit_changed(&app, &instance_id);
    Ok(())
}

/// 清空回收站
#[tauri::command]
pub async fn empty_trash(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<usize, String> {
    let inst = state.paths.instance_dir(&instance_id);
    let n = tauri::async_runtime::spawn_blocking(move || local::empty_trash(&inst))
        .await
        .map_err(|e| format!("任务执行失败：{e}"))?
        .map_err(String::from)?;
    Ok(n)
}

/// 复制外部文件到实例（拖放 / 从文件安装）：模组 .jar → mods/，
/// 数据包 / 光影 / 资源包 .zip → 对应目录。
/// overwrite=false 时同名跳过（前端确认后可带 overwrite=true 重试）。
#[tauri::command]
pub async fn install_local_mods(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    paths: Vec<String>,
    overwrite: bool,
) -> Result<LocalInstallResult, String> {
    let kind = ResourceType::parse(&resource_type)?;
    let inst = state.paths.instance_dir(&instance_id);
    let mc = state.instances.game_dir(&instance_id).await.map_err(String::from)?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut out = LocalInstallResult {
            installed: Vec::new(),
            skipped: Vec::new(),
            failed: Vec::new(),
        };
        let mut sources = ModSources::load(&inst);
        for p in &paths {
            match local::install_local_file(std::path::Path::new(p), &mc, kind, overwrite) {
                Ok((name, true)) => out.skipped.push(name),
                Ok((name, false)) => {
                    sources.insert(
                        kind,
                        &name,
                        ModSource {
                            origin: "local".into(),
                            project_id: None,
                            version_id: None,
                            version_number: None,
                            sha1: None,
                            download_url: None,
                        },
                    );
                    out.installed.push(name);
                }
                Err(_) => out.failed.push(p.clone()),
            }
        }
        if !out.installed.is_empty() {
            if let Err(e) = sources.save(&inst) {
                return Err(e);
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("任务执行失败：{e}"))??;
    emit_changed(&app, &instance_id);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Modrinth 在线安装（批次 B）
// ---------------------------------------------------------------------------

/// 搜索结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchModsResult {
    pub hits: Vec<SearchHit>,
    pub total: u64,
}

/// 在线搜索（origin = modrinth / curseforge；MC 版本与加载器取自实例；
/// category 仅 Modrinth 支持，CurseForge 忽略；CurseForge 仅模组可用）
#[tauri::command]
pub async fn search_mods(
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    origin: String,
    query: String,
    category: Option<String>,
    sort: String,
    offset: u32,
) -> Result<SearchModsResult, String> {
    let kind = ResourceType::parse(&resource_type)?;
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    let mc_version = detail.summary.mc_version.clone();
    let loader = detail.summary.loader_type.clone();

    if origin == "curseforge" {
        if !kind.is_mod() {
            return Err(format!("{}仅支持 Modrinth 来源", kind.label()));
        }
        if loader == "vanilla" {
            return Err("原版实例不支持搜索模组，请先安装 Fabric / Forge 等加载器".into());
        }
        let key = state.settings.read().await.curseforge_api_key.clone();
        let (raw, total) = cf::search(&key, &query, &mc_version, &loader, &sort, offset)
            .await
            .map_err(String::from)?;
        let hits = raw.iter().filter_map(cf_hit).collect();
        return Ok(SearchModsResult { hits, total });
    }

    // 非 mod 类型不按加载器过滤（其 loaders 取值与实例加载器无对应关系）
    let loader_opt = if kind.is_mod() && loader != "vanilla" {
        Some(loader.as_str())
    } else {
        None
    };
    let category = category.filter(|c| !c.is_empty());
    let (hits, total) = modrinth::search(
        &query,
        &mc_version,
        loader_opt,
        category.as_deref(),
        &sort,
        offset,
        kind.modrinth_project_type(),
    )
    .await
    .map_err(String::from)?;
    Ok(SearchModsResult { hits, total })
}

/// 某资源的版本列表（按实例环境过滤，无兼容时回退全量）
#[tauri::command]
pub async fn list_online_versions(
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    origin: String,
    project_id: String,
) -> Result<Vec<OnlineVersion>, String> {
    let kind = ResourceType::parse(&resource_type)?;
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    let mc_version = detail.summary.mc_version.clone();
    let loader = detail.summary.loader_type.clone();

    if origin == "curseforge" {
        if !kind.is_mod() {
            return Err(format!("{}仅支持 Modrinth 来源", kind.label()));
        }
        let key = state.settings.read().await.curseforge_api_key.clone();
        let mod_id: i64 = project_id
            .parse()
            .map_err(|_| format!("非法的 CurseForge 项目 ID：{project_id}"))?;
        let list = cf::files(&key, mod_id, &mc_version, &loader).await.map_err(String::from)?;
        // 兼容项在前
        let mut versions: Vec<OnlineVersion> = list
            .iter()
            .filter(|f| f.compatible_with(&mc_version))
            .map(|f| cf_file_to_version(f, &loader))
            .collect();
        if versions.is_empty() {
            versions = list.iter().map(|f| cf_file_to_version(f, &loader)).collect();
        }
        return Ok(versions);
    }

    modrinth::list_versions(
        &project_id,
        &mc_version,
        &loader,
        kind.modrinth_project_type(),
    )
    .await
    .map_err(String::from)
}

/// 生成安装计划（主文件 + 递归必需依赖 ≤3 层，已安装项被过滤）。
/// 原版实例仅限制模组；数据包 / 光影 / 资源包原版即可安装。
#[tauri::command]
pub async fn plan_mod_install(
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    origin: String,
    version_id: String,
) -> Result<InstallPlan, String> {
    let kind = ResourceType::parse(&resource_type)?;
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    if kind.is_mod() && detail.summary.loader_type == "vanilla" {
        return Err("原版实例不支持安装模组，请先安装 Fabric / Forge 等加载器".into());
    }
    let mc_version = detail.summary.mc_version.clone();
    let loader = detail.summary.loader_type.clone();
    let inst = state.paths.instance_dir(&instance_id);
    let game_dir = state.instances.game_dir(&instance_id).await.map_err(String::from)?;

    // 已安装标识集合：来源记录的 project_id + 本地文件基名（含禁用文件）
    let ext = kind.file_ext();
    let disabled_ext = format!("{ext}.disabled");
    let installed_keys = tauri::async_runtime::spawn_blocking(move || {
        let mut keys = std::collections::HashSet::new();
        let sources = ModSources::load(&inst);
        for (_, s) in sources.iter_kind(kind) {
            if let Some(pid) = &s.project_id {
                keys.insert(pid.clone());
            }
        }
        let dir = game_dir.join(kind.dir_name());
        if let Ok(read) = std::fs::read_dir(&dir) {
            for e in read.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !(name.ends_with(ext) || name.ends_with(&disabled_ext)) {
                    continue;
                }
                let base = name
                    .trim_end_matches(".disabled")
                    .trim_end_matches(ext);
                keys.insert(base.to_string());
                if kind.is_mod() {
                    if let Some(id) = crate::mods::parse::parse_jar(&e.path()).mod_id {
                        keys.insert(id);
                    }
                }
            }
        }
        keys
    })
    .await
    .map_err(|e| format!("任务执行失败：{e}"))?;

    if origin == "curseforge" {
        if !kind.is_mod() {
            return Err(format!("{}仅支持 Modrinth 来源", kind.label()));
        }
        let key = state.settings.read().await.curseforge_api_key.clone();
        let Some((mod_id, file_id)) = version_id.split_once(':') else {
            return Err("CurseForge 安装目标需要 modId:fileId".into());
        };
        let mid: i64 = mod_id
            .parse()
            .map_err(|_| format!("非法的 CurseForge modId：{mod_id}"))?;
        let fid: i64 = file_id
            .parse()
            .map_err(|_| format!("非法的 CurseForge fileId：{file_id}"))?;
        plan_cf(&key, &mc_version, &loader, &installed_keys, mid, fid)
            .await
            .map_err(String::from)
    } else {
        let version = modrinth::get_version(&version_id).await.map_err(String::from)?;
        // 模组校验加载器；数据包 / 光影 / 资源包仅校验 MC 版本
        let compatible = if kind.is_mod() {
            version.compatible_with(&mc_version, &loader)
        } else {
            version.compatible_mc(&mc_version)
        };
        if !compatible {
            return Err(format!(
                "该版本不兼容当前实例（需要 MC {} / {}，实例为 MC {mc_version} / {loader}）",
                version.game_versions.first().map(String::as_str).unwrap_or("?"),
                version.loaders.first().map(String::as_str).unwrap_or("?"),
            ));
        }
        modrinth::resolve_install_plan(&version, &mc_version, &loader, &installed_keys)
            .await
            .map_err(String::from)
    }
}

/// CurseForge 安装计划：文件详情 + 依赖递归（modId → 最新兼容文件，≤3 层）
async fn plan_cf(
    key: &str,
    mc_version: &str,
    loader: &str,
    installed_keys: &std::collections::HashSet<String>,
    mod_id: i64,
    file_id: i64,
) -> Result<InstallPlan, String> {
    let file = cf::get_file(key, mod_id, file_id).await.map_err(String::from)?;
    if !file.compatible_with(mc_version) {
        return Err(format!(
            "该文件不兼容当前实例（支持 {}，实例为 MC {mc_version} / {loader}）",
            file.game_versions.join(", ")
        ));
    }
    let main_spec = cf_file_to_spec(&file)?;
    let infos = cf::get_mod_info(key, &[mod_id]).await.map_err(String::from)?;
    let (slug, title) = infos
        .first()
        .map(|m| {
            (
                m.get("slug").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                m.get("name").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
            )
        })
        .unwrap_or_default();
    let main = InstallPlan {
        main: crate::mods::modrinth::PlanDep {
            project_id: main_spec.project_id.clone(),
            slug,
            title,
            version_id: main_spec.version_id.clone(),
            version_number: main_spec.version_number.clone(),
            file: main_spec.file.clone(),
            already_installed: false,
            origin: "curseforge".into(),
        },
        deps: Vec::new(),
        recommended: Vec::new(),
        unknown_deps: Vec::new(),
    };

    // 依赖 BFS：modId → 最新兼容文件 → 其依赖
    let mut deps: Vec<crate::mods::modrinth::PlanDep> = Vec::new();
    let mut unknown_deps: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut queue: Vec<i64> = cf::required_dep_ids(&file);
    let mut depth = 0;
    while !queue.is_empty() && depth < 3 {
        let batch: Vec<i64> = queue
            .drain(..)
            .filter(|id| seen.insert(*id))
            .collect();
        if batch.is_empty() {
            break;
        }
        let infos = cf::get_mod_info(key, &batch).await.map_err(String::from)?;
        let mut next: Vec<i64> = Vec::new();
        for dep_id in batch {
            let key_s = dep_id.to_string();
            if installed_keys.contains(&key_s) {
                continue;
            }
            let (slug, title) = infos
                .iter()
                .find(|m| m.get("id").and_then(|x| x.as_i64()) == Some(dep_id))
                .map(|m| {
                    (
                        m.get("slug").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                        m.get("name").and_then(|x| x.as_str()).unwrap_or_default().to_string(),
                    )
                })
                .unwrap_or_default();
            let Some(dep_file) = cf::resolve_latest(key, dep_id, mc_version, loader)
                .await
                .map_err(String::from)?
            else {
                unknown_deps.push(if title.is_empty() {
                    format!("CurseForge 项目 {dep_id}")
                } else {
                    title.clone()
                });
                continue;
            };
            // 展开下一层依赖
            next.extend(cf::required_dep_ids(&dep_file));
            if installed_keys.contains(&slug) {
                continue;
            }
            let Ok(spec) = cf_file_to_spec(&dep_file) else {
                unknown_deps.push(if title.is_empty() {
                    format!("CurseForge 项目 {dep_id}")
                } else {
                    title.clone()
                });
                continue;
            };
            deps.push(crate::mods::modrinth::PlanDep {
                project_id: spec.project_id.clone(),
                slug,
                title,
                version_id: spec.version_id.clone(),
                version_number: spec.version_number.clone(),
                file: spec.file.clone(),
                already_installed: false,
                origin: "curseforge".into(),
            });
        }
        queue = next;
        depth += 1;
    }

    Ok(InstallPlan {
        deps: deps
            .into_iter()
            .filter(|d| d.version_id != main.main.version_id)
            .collect(),
        recommended: Vec::new(),
        unknown_deps,
        ..main
    })
}

/// 执行安装（confirm）：targets 为统一格式（`mr:<version_id>` / `cf:<mod_id>:<file_id>`），
/// 批量下载 primary 文件到类型对应目录（sha1 校验），replace_file_names 用于更新场景移除旧文件。
/// 返回实际安装的文件名列表。
#[tauri::command]
pub async fn confirm_mod_install(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
    version_ids: Vec<String>,
    replace_file_names: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    confirm_install_core(
        &app,
        &state,
        &instance_id,
        &resource_type,
        &version_ids,
        replace_file_names,
    )
    .await
}

/// confirm_mod_install 的共享核心（命令与创建实例的附加组件安装共用）
pub(crate) async fn confirm_install_core(
    app: &AppHandle,
    state: &AppState,
    instance_id: &str,
    resource_type: &str,
    version_ids: &[String],
    replace_file_names: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let kind = ResourceType::parse(resource_type)?;
    let detail = state.instances.get(instance_id).await.map_err(String::from)?;
    if kind.is_mod() && detail.summary.loader_type == "vanilla" {
        return Err("原版实例不支持安装模组，请先安装 Fabric / Forge 等加载器".into());
    }
    let mc_version = detail.summary.mc_version.clone();
    let loader = detail.summary.loader_type.clone();
    let cf_key = state.settings.read().await.curseforge_api_key.clone();

    // 解析各平台目标 → 统一下载规格（含兼容性校验）
    let mut specs: Vec<DlSpec> = Vec::new();
    for t in version_ids {
        let (origin, pair) = parse_target(t)?;
        match (origin, pair) {
            ("modrinth", Some((id, _))) => {
                let v = modrinth::get_version(id).await.map_err(String::from)?;
                let compatible = if kind.is_mod() {
                    v.compatible_with(&mc_version, &loader)
                } else {
                    v.compatible_mc(&mc_version)
                };
                if !compatible {
                    return Err(format!(
                        "版本 {} 不兼容当前实例（MC {mc_version} / {loader}）",
                        v.version_number
                    ));
                }
                let Some(f) = v.primary_file() else {
                    return Err(format!("版本 {} 没有可下载的文件", v.version_number));
                };
                specs.push(DlSpec {
                    origin: "modrinth".into(),
                    project_id: v.project_id.clone(),
                    version_id: v.id.clone(),
                    version_number: v.version_number.clone(),
                    file: f.clone(),
                });
            }
            ("curseforge", Some((m, fid))) => {
                if !kind.is_mod() {
                    return Err(format!("{}仅支持 Modrinth 来源", kind.label()));
                }
                let mid: i64 = m
                    .parse()
                    .map_err(|_| format!("非法的 CurseForge modId：{m}"))?;
                let fid: i64 = fid
                    .parse()
                    .map_err(|_| format!("非法的 CurseForge fileId：{fid}"))?;
                let f = cf::get_file(&cf_key, mid, fid).await.map_err(String::from)?;
                if !f.compatible_with(&mc_version) {
                    return Err(format!(
                        "文件 {} 不兼容当前实例（MC {mc_version}）",
                        f.display_name
                    ));
                }
                specs.push(cf_file_to_spec(&f)?);
            }
            _ => return Err(format!("未知的安装目标：{t}")),
        }
    }

    let inst = state.paths.instance_dir(instance_id);
    let mc_dir = state.instances.game_dir(instance_id).await.map_err(String::from)?;
    let retention = state.settings.read().await.trash_retention_days;

    // 更新场景：先把旧文件移入回收站（与旧文件同名的新文件不会被误删——
    // 若 replace 项与新文件同名则跳过删除，由下载覆盖）
    let new_names: Vec<String> = specs.iter().map(|s| s.file.filename.clone()).collect();
    if let Some(replaces) = &replace_file_names {
        let keep: Vec<String> = replaces
            .iter()
            .filter(|r| !new_names.contains(r))
            .cloned()
            .collect();
        if !keep.is_empty() {
            local::delete_mods(&inst, &mc_dir, kind, &keep, retention).map_err(String::from)?;
        }
    }

    // 构造下载请求（primary 文件 → 类型对应目录），每个文件一个独立下载任务：
    // 用户要求各文件分别显示自己的进度（行标题 = 文件名），而非合并成一个任务
    let target_dir = mc_dir.join(kind.dir_name());
    let mut rxs = Vec::new();
    for s in &specs {
        let mut req = DownloadRequest::new(
            s.file.filename.clone(),
            crate::download::mirror::market_candidates(std::slice::from_ref(&s.file.url)),
            target_dir.join(&s.file.filename),
        );
        if let Some(sha) = &s.file.sha1 {
            req.expected = Some((HashAlgo::Sha1, sha.clone()));
        }
        req.size = Some(s.file.size);

        let task_id = format!("mods-{}", uuid::Uuid::new_v4().simple());
        let opts = crate::download::TaskOptions {
            task_id: task_id.clone(),
            kind: "mods".into(),
            instance_id: Some(instance_id.to_string()),
        };
        super::register_retry(&state.retry_defs, &task_id, opts.clone(), vec![req.clone()], None);

        // 立即推送一条初始进度：确认后任务行即时出现（版本解析等 API 请求期间不空白）
        let _ = app.emit(
            "download-progress",
            &crate::download::model::DownloadProgress {
                task_id: task_id.clone(),
                kind: "mods".into(),
                instance_id: Some(instance_id.to_string()),
                state: "running".into(),
                current_file: Some(s.file.filename.clone()),
                completed_files: 0,
                total_files: 1,
                downloaded_bytes: 0,
                total_bytes: s.file.size,
                bytes_per_second: 0.0,
                error: None,
            },
        );

        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let tx = std::sync::Mutex::new(Some(tx));
        let cb: ProgressCallback = super::make_retry_cb(
            state.retry_defs.clone(),
            state.active_downloads.clone(),
            app.clone(),
            task_id.clone(),
            Some(tx),
        );
        let start = state.download.start_task(opts, vec![req], cb).await;
        if let Err(e) = start {
            super::remove_retry(&state.retry_defs, &task_id);
            return Err(e);
        }
        rxs.push(rx);
    }

    // 等待全部任务终态（任务已并行执行，逐个收取结果）
    for rx in rxs {
        rx.await
            .map_err(|_| "模组下载任务意外结束".to_string())?
            .map_err(String::from)?;
    }

    // 写来源记录（按平台 origin 区分，key 带类型前缀）
    let mut sources = ModSources::load(&inst);
    for s in &specs {
        sources.insert(
            kind,
            &s.file.filename,
            ModSource {
                origin: s.origin.clone(),
                project_id: Some(s.project_id.clone()),
                version_id: Some(s.version_id.clone()),
                version_number: Some(s.version_number.clone()),
                sha1: s.file.sha1.clone(),
                download_url: Some(s.file.url.clone()),
            },
        );
    }
    sources.save(&inst).map_err(String::from)?;

    emit_changed(app, instance_id);
    Ok(new_names)
}

/// Fabric / Quilt 实例附加组件一键安装（创建实例时勾选触发）：
/// - with_iris_sodium：Iris + Sodium（光影 + 优化），Iris 的必需依赖
///   （Sodium / Fabric API）由依赖树自动带出
/// - with_api：Fabric API 最新兼容版（若已被 Iris 依赖树带出则不重复解析）
///
/// 各组件解析最新兼容版本后走共享安装核心（mods 目录 + 来源记录，
/// 资源管理页可见可管理）。返回实际安装的文件名列表。
#[tauri::command]
pub async fn install_fabric_addons(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    with_api: bool,
    with_iris_sodium: bool,
) -> Result<Vec<String>, String> {
    if !with_api && !with_iris_sodium {
        return Ok(vec![]);
    }
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    let mc_version = detail.summary.mc_version.clone();
    let loader = detail.summary.loader_type.clone();
    if loader != "fabric" && loader != "quilt" {
        return Err("附加组件仅支持 Fabric / Quilt 实例".into());
    }

    // 兼容版本解析：Quilt 实例对仅标注 fabric 的项目回落 fabric 加载器过滤
    async fn pick_latest(
        project: &str,
        mc_version: &str,
        loader: &str,
    ) -> Result<OnlineVersion, String> {
        if let Some(v) = modrinth::latest_compatible(project, mc_version, loader, "mod")
            .await
            .map_err(String::from)?
        {
            return Ok(v);
        }
        if loader == "quilt" {
            if let Some(v) = modrinth::latest_compatible(project, mc_version, "fabric", "mod")
                .await
                .map_err(String::from)?
            {
                return Ok(v);
            }
        }
        Err(format!("未找到与 MC {mc_version} 兼容的 {project} 版本"))
    }

    let mut targets: Vec<String> = Vec::new();
    let mut project_ids: Vec<String> = Vec::new();

    if with_iris_sodium {
        let v = pick_latest("iris", &mc_version, &loader).await?;
        let plan =
            modrinth::resolve_install_plan(&v, &mc_version, &loader, &HashSet::new())
                .await
                .map_err(String::from)?;
        targets.push(format!("mr:{}", plan.main.version_id));
        project_ids.push(plan.main.project_id.clone());
        for d in &plan.deps {
            targets.push(format!("mr:{}", d.version_id));
            project_ids.push(d.project_id.clone());
        }
    }
    if with_api && !project_ids.iter().any(|p| p == FABRIC_API_PID) {
        let v = pick_latest("fabric-api", &mc_version, &loader).await?;
        targets.push(format!("mr:{}", v.id));
    }

    if targets.is_empty() {
        return Ok(vec![]);
    }

    // 用户决策：附加组件等游戏本体资源下载完成后再入队——家庭带宽（100M 级）
    // 跑满时两个下载流互相挤占会双双失败。plan 阶段已并行完成（API 流量小），
    // 此处等待实例级资源任务结束（引擎完成回调释放 active_downloads 键）再下载。
    const WAIT_MAX_SECS: u64 = 15 * 60;
    let mut waited: u64 = 0;
    while state
        .active_downloads
        .lock()
        .unwrap()
        .contains_key(&instance_id)
    {
        if waited >= WAIT_MAX_SECS {
            return Err(
                "游戏资源下载尚未完成，附加组件已停止等待；可在实例「资源管理」页手动重装"
                    .into(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        waited += 2;
    }

    confirm_install_core(&app, &state, &instance_id, "mod", &targets, None).await
}

// ---------------------------------------------------------------------------
// OptiFine（Forge 实例附加组件）
// ---------------------------------------------------------------------------

/// OptiFine 与 Forge 的混装方式（OptiDocs 2026 官方推荐，与 HMCL / PCL 现行方式
/// 一致）：OptiFine 1.13+ 的 jar 内置 Forge 兼容层，**直接放入 mods 文件夹即可**，
/// 无需运行 installer 或合并版本 json。下载仅能走官方 optifine.net
/// （OptiFine 分发协议禁止第三方镜像，BMCLAPI 仅提供版本列表）。

/// OptiFine 版本条目（list_optifine_forge）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptiFineEntry {
    /// 类型，如 HD_U
    pub optifine_type: String,
    /// 补丁号，如 I6
    pub patch: String,
    /// 官方文件名 OptiFine_{mc}_{type}_{patch}.jar
    pub filename: String,
    /// 该版本要求的最低 Forge 构建（BMCLAPI 元数据，如 "47.0.35"）
    pub min_forge: Option<String>,
    /// 与当前实例 Forge 的兼容性（要求最低 ≤ 实例 build）
    pub compatible: bool,
}

/// 数字段比较：a ≤ b（缺省段视为 0）
fn version_le(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c| c == '.' || c == '-')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let (x, y) = (
            va.get(i).copied().unwrap_or(0),
            vb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x < y;
        }
    }
    true
}

/// 列出 OptiFine 版本并标记与目标 Forge 构建的兼容性（BMCLAPI 版本列表）。
/// forge_build 传实例当前 Forge build（创建对话框场景可传下拉选中的 build 以便
/// 创建前预览兼容性；None 时全部标记兼容）。
#[tauri::command]
pub async fn list_optifine_forge(
    mc_version: String,
    forge_build: Option<String>,
) -> Result<Vec<OptiFineEntry>, String> {
    let forge_build = forge_build.unwrap_or_default();

    let http = crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent("PieMCL/0.1")
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30)))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))?;
    let list: Value = http
        .get(format!("https://bmclapi2.bangbang93.com/optifine/{mc_version}"))
        .send()
        .await
        .map_err(|e| format!("获取 OptiFine 版本列表失败：{e}"))?
        .json()
        .await
        .map_err(|e| format!("解析 OptiFine 版本列表失败：{e}"))?;

    let mut out: Vec<OptiFineEntry> = Vec::new();
    if let Some(arr) = list.as_array() {
        for e in arr {
            let (Some(t), Some(p)) = (
                e.get("type").and_then(|v| v.as_str()),
                e.get("patch").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            // "Forge 47.0.35" → 最低 Forge build
            let min_forge = e
                .get("forge")
                .and_then(|v| v.as_str())
                .and_then(|s| s.strip_prefix("Forge "))
                .map(String::from);
            let compatible = match &min_forge {
                Some(mf) => version_le(mf, &forge_build),
                None => true,
            };
            out.push(OptiFineEntry {
                optifine_type: t.to_string(),
                patch: p.to_string(),
                filename: format!("OptiFine_{mc_version}_{t}_{p}.jar"),
                min_forge,
                compatible,
            });
        }
    }
    // 最新在前（patch 段降序近似）
    out.sort_by(|a, b| b.patch.cmp(&a.patch));
    Ok(out)
}

/// 安装 OptiFine 到 Forge 实例（jar 放入 mods 目录 + 来源记录 origin=optifine）。
/// 下载走官方 optifine.net：adloadx 页面 → 解析 downloadx 一次性链接 → jar。
#[tauri::command]
pub async fn install_optifine(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    optifine_type: String,
    patch: String,
) -> Result<String, String> {
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    if detail.summary.loader_type != "forge" {
        return Err("OptiFine 仅支持安装到 Forge 实例".into());
    }
    let mc_version = detail.summary.mc_version.clone();
    let filename = format!("OptiFine_{mc_version}_{optifine_type}_{patch}.jar");

    let http = crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent("PieMCL/0.1")
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120)))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))?;

    // 1. adloadx 页面（官方下载入口）
    let page_url = format!("https://optifine.net/adloadx?f={filename}");
    let html = http
        .get(&page_url)
        .send()
        .await
        .map_err(|e| format!("访问 OptiFine 官网失败：{e}"))?
        .text()
        .await
        .map_err(|e| format!("读取 OptiFine 官网页面失败：{e}"))?;

    // 2. 解析 downloadx 一次性链接（token 链接有时效，需立即下载）
    let token_start = html
        .find("downloadx?f=")
        .ok_or("OptiFine 官方页面未找到下载链接（可能暂时不可用，请稍后重试）")?;
    let token_end_rel = html[token_start..]
        .find('"')
        .ok_or("OptiFine 下载链接解析失败")?;
    let link = html[token_start..token_start + token_end_rel].replace("&amp;", "&");
    let dl_url = format!("https://optifine.net/{link}");

    // 3. 下载 jar 到实例 mods 目录（OptiFine 禁止镜像，单流直写即可）
    let game_dir = state.instances.game_dir(&instance_id).await.map_err(String::from)?;
    let dest = game_dir.join("mods").join(&filename);
    let bytes = http
        .get(&dl_url)
        .send()
        .await
        .map_err(|e| format!("下载 OptiFine 失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("下载 OptiFine 失败：{e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取 OptiFine 数据失败：{e}"))?;
    if bytes.len() < 1024 * 1024 {
        return Err("下载的 OptiFine 文件异常（过小），请稍后重试".into());
    }
    tokio::fs::write(&dest, &bytes)
        .await
        .map_err(|e| format!("写入 OptiFine 失败：{e}"))?;

    // 4. 来源记录（资源管理页可见；更新检查自然跳过非 modrinth/cf 来源）
    let inst = state.paths.instance_dir(&instance_id);
    let mut sources = ModSources::load(&inst);
    sources.insert(
        ResourceType::Mod,
        &filename,
        ModSource {
            origin: "optifine".into(),
            project_id: None,
            version_id: None,
            version_number: Some(format!("{optifine_type}-{patch}")),
            sha1: None,
            download_url: Some(dl_url),
        },
    );
    sources.save(&inst).map_err(String::from)?;

    emit_changed(&app, &instance_id);
    Ok(filename)
}

/// 可更新条目（check_mod_updates）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 来源平台：modrinth / curseforge
    pub origin: String,
    /// 现有文件名（更新确认时作为 replace 项）
    pub file_name: String,
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub current_version_number: String,
    pub new_version_number: String,
    /// 统一安装目标（mr:... / cf:...），直接传给 confirm_mod_install
    pub new_version_id: String,
}

/// 图标 URL 白名单后缀（仅代理已知 CDN，防止被当作任意网络代理）
/// minecraft.net 覆盖 textures.minecraft.net（正版皮肤 / 披风贴图，皮肤 3D 预览复用）；
/// mc-heads / crafatar / minotar 为玩家头像 API（首页账户卡头像）
const ICON_HOST_SUFFIXES: &[&str] = &[
    "modrinth.com",
    "forgecdn.net",
    "curseforge.com",
    "minecraft.net",
    "mc-heads.net",
    "crafatar.com",
    "minotar.net",
    "nmsr.nickac.dev",
];

/// MIME 按扩展名推断（默认 png）
fn icon_mime(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/png"
    }
}

/// 图标 URL 白名单校验（仅代理已知 CDN，防止被当作任意网络代理）。
/// textures.minecraft.net 的皮肤 / 披风贴图官方返回 http:// 地址，故白名单域名内 http 亦放行
fn icon_host_ok(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or("");
    if rest.is_empty() {
        return false;
    }
    let host = rest.split('/').next().unwrap_or("");
    ICON_HOST_SUFFIXES
        .iter()
        .any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

/// 模组图标代理：经 Rust 下载（绕开 WebView 直连 CDN 的慢/失败问题），
/// 缓存于 {root}/cache/icons/{sha1(url)}，命中缓存秒回。
/// 返回 data:image/...;base64 data URL。
#[tauri::command]
pub async fn fetch_icon(state: State<'_, AppState>, url: String) -> Result<String, String> {
    eprintln!("[icon] 请求：{url}");
    if !icon_host_ok(&url) {
        eprintln!("[icon] 白名单拒绝：{url}");
        return Err(format!("不支持的图标地址：{url}"));
    }

    // 缓存键 = sha1(url)
    use sha1::Digest;
    let key = hex::encode(sha1::Sha1::digest(url.as_bytes()));
    let cache_dir = state.paths.cache_dir().join("icons");
    let dest = cache_dir.join(&key);

    let bytes = match tokio::fs::read(&dest).await {
        Ok(b) => {
            eprintln!("[icon] 缓存命中：{}（{} 字节）", key, b.len());
            b
        }
        Err(_) => {
            // 下载（带超时，图标通常 <100KB）：官方源优先（数量多、延迟敏感），
            // MCIMirror 兜底
            let client = crate::core::net::apply_proxy(reqwest::Client::builder()
                .user_agent(concat!("PieMCL/", env!("CARGO_PKG_VERSION")))
                .timeout(std::time::Duration::from_secs(15)))
                .build()
                .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))?;
            let candidates =
                crate::download::mirror::market_candidates(std::slice::from_ref(&url));
            let mut data: Option<Vec<u8>> = None;
            let mut last_err: Option<String> = None;
            // 两轮尝试（官方 → 镜像 → 官方 → 镜像）：本机到官方 CDN 的连接
            // 时好时坏，单轮失败常见，第二轮往往即可命中
            for round in 0..2 {
                for u in &candidates {
                    match client
                        .get(u)
                        .send()
                        .await
                        .and_then(|r| r.error_for_status())
                    {
                        Ok(resp) => match resp.bytes().await {
                            Ok(b) => {
                                data = Some(b.to_vec());
                                break;
                            }
                            Err(e) => last_err = Some(format!("读取响应失败：{e}")),
                        },
                        Err(e) => last_err = Some(format!("{e}")),
                    }
                }
                if data.is_some() {
                    break;
                }
                if round == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
            }
            let data = data.ok_or_else(|| {
                eprintln!("[icon] 下载失败：{url} → {:?}", last_err);
                format!("下载图标失败：{}", last_err.unwrap_or_default())
            })?;
            let _ = tokio::fs::create_dir_all(&cache_dir).await;
            let _ = tokio::fs::write(&dest, &data).await;
            eprintln!("[icon] 下载成功：{url}（{} 字节）", data.len());
            data
        }
    };
    Ok(format!(
        "data:{};base64,{}",
        icon_mime(&url),
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
    ))
}

/// 检查更新（模组：Modrinth + CurseForge（需已配置 key）；其余类型仅 Modrinth）
#[tauri::command]
pub async fn check_mod_updates(
    state: State<'_, AppState>,
    instance_id: String,
    resource_type: String,
) -> Result<Vec<UpdateInfo>, String> {
    let kind = ResourceType::parse(&resource_type)?;
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    if kind.is_mod() && detail.summary.loader_type == "vanilla" {
        return Ok(Vec::new());
    }
    let mc_version = detail.summary.mc_version.clone();
    let loader = detail.summary.loader_type.clone();
    let cf_key = state.settings.read().await.curseforge_api_key.clone();
    let inst = state.paths.instance_dir(&instance_id);

    let sources = ModSources::load(&inst);
    let targets: Vec<(String, ModSource)> = sources
        .iter_kind(kind)
        .filter(|(_, s)| {
            s.project_id.is_some()
                && (s.origin == "modrinth"
                    || (kind.is_mod()
                        && s.origin == "curseforge"
                        && !cf_key.trim().is_empty()))
        })
        .map(|(k, s)| (k.to_string(), s.clone()))
        .collect();

    let mut out = Vec::new();
    for (file_name, s) in targets {
        let pid = s.project_id.clone().expect("已过滤");
        let (origin, new_number, new_target) = if s.origin == "modrinth" {
            let Ok(Some(latest)) = modrinth::latest_compatible(
                &pid,
                &mc_version,
                &loader,
                kind.modrinth_project_type(),
            )
            .await
            else {
                continue;
            };
            if latest.id == s.version_id.clone().unwrap_or_default() {
                continue;
            }
            (
                "modrinth".to_string(),
                latest.version_number.clone(),
                format!("mr:{}", latest.id),
            )
        } else {
            let Ok(mid) = pid.parse::<i64>() else { continue };
            let Ok(Some(latest)) = cf::resolve_latest(&cf_key, mid, &mc_version, &loader).await
            else {
                continue;
            };
            if latest.id.to_string() == s.version_id.clone().unwrap_or_default() {
                continue;
            }
            (
                "curseforge".to_string(),
                latest.display_name.clone(),
                format!("cf:{pid}:{}", latest.id),
            )
        };
        // 项目标题 / slug（失败时降级用 project_id 展示）
        let (slug, title) = if origin == "modrinth" {
            modrinth::get_projects_batch(std::slice::from_ref(&pid))
                .await
                .ok()
                .and_then(|mut v| v.pop().map(|(_, s, t)| (s, t)))
                .unwrap_or((pid.clone(), pid.clone()))
        } else {
            let mid = pid.parse::<i64>().unwrap_or(0);
            cf::get_mod_info(&cf_key, &[mid])
                .await
                .ok()
                .and_then(|arr| {
                    arr.first().and_then(|m| {
                        Some((
                            m.get("slug")?.as_str()?.to_string(),
                            m.get("name")?.as_str()?.to_string(),
                        ))
                    })
                })
                .unwrap_or((pid.clone(), pid.clone()))
        };
        out.push(UpdateInfo {
            origin,
            file_name,
            project_id: pid,
            slug,
            title,
            current_version_number: s.version_number.clone().unwrap_or_default(),
            new_version_number: new_number,
            new_version_id: new_target,
        });
    }
    Ok(out)
}
