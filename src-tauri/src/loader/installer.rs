// Forge / NeoForge installer 安装管线（install_profile spec=1）
//
// 流程：原版资源补全 → installer.jar 下载 → 解析 profile/version →
//       解压内置 maven/ → 下载 processor 依赖 → 执行 processors（重映射/binpatch）→
//       校验 PATCHED 产物 → 复制为主 jar → 合并 version.json 写回实例。
//
// 关键实现依据（Forge 1.20.1-47.4.28 实测 + NeoForge 同构）：
// - data 值四种格式：[maven坐标[:classifier][@ext]] / {KEY} 递归引用 /
//   /data/xxx（installer 内文件） / 'sha1'（带引号字面量，替换时去引号）
// - processor 跳过条件：sides 存在且不含 "client"；主类取 profile 的 main
//   字段或 processor.jar 的 MANIFEST Main-Class
// - 启动 classpath 需要：patch 后主 jar（复制到 versions/{vid}/{vid}.jar，
//   由 build_classpath 固定规则定位）+ client-extra（jarsplitter 产物，
//   installer version.json 不含该条目，收尾时按 MC_EXTRA 坐标手动追加）

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::core::error::{AppError, AppResult};
use crate::download::model::{DownloadRequest, DownloadProgress, HashAlgo};
use crate::download::{DownloadEngine, TaskOptions};
use crate::game::planner;
use crate::commands::{register_retry, remove_retry, RetryMap};

/// BMCLAPI 对 Forge maven 的镜像根
const BMCLAPI_MAVEN: &str = "https://bmclapi2.bangbang93.com/maven";
/// Forge 官方 maven
const FORGE_MAVEN: &str = "https://maven.minecraftforge.net/maven";
/// NeoForge 官方 maven（无镜像）
const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases";

/// installer 下载地址（full = 版本列表返回的 build 号，如 "47.4.28" / "10.13.0.1151"；
/// NeoForge 为官方版本号 "21.1.95"）
fn installer_urls(loader_type: &str, mc_version: &str, full: &str) -> Vec<String> {
    match loader_type {
        "forge" => {
            // Forge maven 目录为 {mc}-{build}；部分老版 build（如 1.7.10 后期
            // 10.13.4.1614）实际发布于带 branch 后缀的 {mc}-{build}-{mc} 目录，
            // 两种变体都作为候选
            let base = if full.starts_with(&format!("{mc_version}-")) {
                full.to_string()
            } else {
                format!("{mc_version}-{full}")
            };
            let variants = [
                base.clone(),
                format!("{base}-{mc_version}"),
            ];
            let mut candidates: Vec<String> = Vec::new();
            for v in variants {
                let path = format!("net/minecraftforge/forge/{v}/forge-{v}-installer.jar");
                candidates.push(format!("{BMCLAPI_MAVEN}/{path}"));
                candidates.push(format!("{FORGE_MAVEN}/{path}"));
            }
            candidates
        }
        _ => {
            // NeoForge：1.20.1 特例沿用 net/neoforged/forge 命名（版本号 47.1.x），
            // 其余为 net/neoforged/neoforge；官方 maven 国内不可达时回落 BMCLAPI 镜像
            let path = if mc_version == "1.20.1" {
                format!("net/neoforged/forge/{full}/forge-{full}-installer.jar")
            } else {
                format!("net/neoforged/neoforge/{full}/neoforge-{full}-installer.jar")
            };
            vec![
                format!("{BMCLAPI_MAVEN}/{path}"),
                format!("{NEOFORGE_MAVEN}/{path}"),
            ]
        }
    }
}

/// Maven 坐标 → 库相对路径（支持 classifier 与 @ext，如 "g:a:v:class@txt"）
fn coords_to_rel(coords: &str) -> AppResult<String> {
    let (coords, ext) = match coords.split_once('@') {
        Some((c, e)) => (c, e),
        None => (coords, "jar"),
    };
    planner::maven_path_from_name(coords)
        .map(|rel| {
            // maven_path_from_name 固定 .jar 后缀，这里替换为实际扩展名
            match rel.strip_suffix(".jar") {
                Some(base) => format!("{base}.{ext}"),
                None => rel,
            }
        })
        .ok_or_else(|| AppError::msg(format!("无法解析 Maven 坐标：{coords}")))
}

/// 安装依赖句柄（由命令层打包）。
/// app 为 Option：真实运行传入 Some（推送进度/事件），测试环境传 None（静默，
/// Java 已装时不走补装分支），使全管线可被无 UI 测试驱动。
pub struct ForgeInstallCtx {
    pub app: Option<AppHandle>,
    pub download: DownloadEngine,
    pub active: Arc<StdMutex<HashMap<String, String>>>,
    pub retry_defs: RetryMap,
    pub task_id: String,
    pub instance_id: String,
    /// 启动器根目录（Java 自动补装的检测范围依赖它）
    pub paths: crate::core::paths::LauncherPaths,
    /// 游戏公共根（.minecraft）：libraries / assets / versions 共享区
    pub game_root: PathBuf,
    /// 实例冻结的原版 version.json
    pub version_json: Value,
    pub loader_type: String,
    /// 完整版本号："1.20.1-47.4.28"（Forge）/ "21.1.95"（NeoForge 1.20.2+）
    pub full_version: String,
    pub mc_version: String,
    pub mirrors: Vec<String>,
    pub official_first: bool,
}

impl ForgeInstallCtx {
    /// 测试/开发工具构造空的任务互斥表与重试登记表（RetryDef 类型不外露）
    pub fn new_maps(
    ) -> (
        Arc<StdMutex<HashMap<String, String>>>,
        crate::commands::RetryMap,
    ) {
        (
            Arc::new(StdMutex::new(HashMap::new())),
            Arc::new(StdMutex::new(HashMap::new())),
        )
    }
}

/// 推送 processor 阶段的手动进度（复用下载队列行，state=running）
fn emit_stage(ctx: &ForgeInstallCtx, message: &str) {
    let Some(app) = &ctx.app else { return };
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            task_id: ctx.task_id.clone(),
            kind: "loader".into(),
            instance_id: Some(ctx.instance_id.clone()),
            state: "running".into(),
            current_file: Some(message.into()),
            completed_files: 0,
            total_files: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            bytes_per_second: 0.0,
            error: None,
        },
    );
}

/// 串行下载一组请求（oneshot 等待终态；沿用 pack.rs 的重试登记模式）
async fn download_requests(
    ctx: &ForgeInstallCtx,
    requests: Vec<DownloadRequest>,
) -> Result<(), String> {
    if requests.is_empty() {
        return Ok(());
    }
    let task_id = ctx.task_id.clone();
    let opts = TaskOptions {
        task_id: task_id.clone(),
        kind: "loader".into(),
        instance_id: Some(ctx.instance_id.clone()),
    };
    register_retry(
        &ctx.retry_defs,
        &task_id,
        opts,
        requests.clone(),
        None,
    );
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let tx = StdMutex::new(Some(tx));
    // 进度回调：emit 走 Option（测试静默），终态管理与 make_retry_cb 相同
    let app = ctx.app.clone();
    let retry_defs = ctx.retry_defs.clone();
    let active = ctx.active.clone();
    let tid = task_id.clone();
    let cb: crate::download::ProgressCallback = Arc::new(move |p| {
        if let Some(app) = &app {
            let _ = app.emit("download-progress", &p);
        }
        if matches!(p.state.as_str(), "completed" | "failed" | "cancelled") {
            if p.state == "completed" {
                retry_defs.lock().unwrap().remove(&tid);
            }
            if let Some(def) = retry_defs.lock().unwrap().get(&tid) {
                if let Some(key) = &def.lock_key {
                    active.lock().unwrap().remove(key);
                }
            }
            if let Some(s) = tx.lock().unwrap().take() {
                let _ = s.send(if p.state == "completed" {
                    Ok(())
                } else {
                    Err(p.error.clone().unwrap_or_else(|| "下载失败".into()))
                });
            }
        }
    });
    let start = ctx
        .download
        .start_task(
            TaskOptions {
                task_id,
                kind: "loader".into(),
                instance_id: Some(ctx.instance_id.clone()),
            },
            requests,
            cb,
        )
        .await;
    if let Err(e) = start {
        remove_retry(&ctx.retry_defs, &ctx.task_id);
        return Err(e);
    }
    rx.await
        .map_err(|_| "下载任务意外结束".to_string())?
        .map_err(String::from)
}

/// 安装入口（调用方持有实例互斥锁并负责释放）
pub async fn install_forge_like(ctx: ForgeInstallCtx) -> Result<(), String> {
    let libs_dir = ctx.game_root.join("libraries");
    let work_dir = ctx.paths.cache_dir().join("forge").join(&ctx.full_version);
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|e| format!("创建安装工作目录失败：{e}"))?;

    // ---- 1. 原版资源补全（processor 输入依赖原版 client.jar；增量跳过）----
    emit_stage(&ctx, "正在检查原版游戏资源…");
    ensure_base_resources(&ctx).await?;

    // ---- 1.5 对齐官方流程：processors（jarsplitter）以 {MINECRAFT_JAR} =
    // versions/{实例id}/{实例id}.jar 为输入，需在 processors 前就位。
    // 先复制原版 client.jar 占位，PATCHED 完成后再覆盖为补丁产物。
    // （缺失该步骤时全新安装会在 jarsplitter 报 FileNotFoundException）
    {
        let mc_id = ctx
            .version_json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(&ctx.mc_version)
            .to_string();
        let vanilla_jar = ctx
            .game_root
            .join("versions")
            .join(&mc_id)
            .join(format!("{mc_id}.jar"));
        let placeholder = ctx
            .paths
            .instance_dir(&ctx.instance_id)
            .join(format!("{}.jar", ctx.instance_id));
        if vanilla_jar.is_file() && !placeholder.is_file() {
            if let Some(parent) = placeholder.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| format!("创建版本目录失败：{e}"))?;
            }
            tokio::fs::copy(&vanilla_jar, &placeholder)
                .await
                .map_err(|e| format!("复制原版 client.jar 失败：{e}"))?;
        }
    }

    // ---- 2. installer.jar 下载（全局缓存复用）----
    let installer_jar = work_dir.join(format!("{}-installer.jar", ctx.full_version));
    if !installer_jar.is_file() {
        emit_stage(&ctx, &format!("正在下载 {} 安装器…", ctx.full_version));
        let urls = installer_urls(&ctx.loader_type, &ctx.mc_version, &ctx.full_version);
        let req = DownloadRequest::new(
            format!("installer/{}", ctx.full_version),
            urls,
            installer_jar.clone(),
        );
        download_requests(&ctx, vec![req]).await?;
    }

    // ---- 3. 解析 installer（zip 内 install_profile.json / version.json）----
    emit_stage(&ctx, "正在解析安装器…");
    let jar_path = installer_jar.clone();
    let parsed = tauri::async_runtime::spawn_blocking(move || -> AppResult<(Value, Value)> {
        let file = std::fs::File::open(&jar_path)?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| AppError::msg(format!("打开安装器失败：{e}")))?;
        let profile: Value = {
            let mut f = zip
                .by_name("install_profile.json")
                .map_err(|_| AppError::msg("installer 缺少 install_profile.json"))?;
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            serde_json::from_str(&s)?
        };
        // version.json：独立文件优先；缺失时回落 install_profile.versionInfo
        // （部分版本的 installer 把版本信息内嵌在 install_profile 中）
        let version: Value = match zip.by_name("version.json") {
            Ok(mut f) => {
                let mut s = String::new();
                f.read_to_string(&mut s)?;
                serde_json::from_str(&s)?
            }
            Err(_) => profile
                .get("versionInfo")
                .cloned()
                .ok_or_else(|| {
                    AppError::msg("installer 缺少 version.json，且 install_profile 无 versionInfo")
                })?,
        };
        Ok((profile, version))
    })
    .await
    .map_err(|e| format!("解析任务执行失败：{e}"))??;

    let (profile, installer_version) = parsed;
    let modern = profile
        .get("processors")
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    // ---- 旧版安装器（≤1.12.2：无 processors，launchwrapper + universal 解压式）----
    if !modern {
        return install_legacy_forge(&ctx, &profile, &installer_version, &libs_dir).await;
    }

    // ---- 4. 解压 installer 内置 maven/ 库 ----
    emit_stage(&ctx, "正在解压安装器内置库…");
    let maven_jar = installer_jar.clone();
    let libs2 = libs_dir.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let file = std::fs::File::open(&maven_jar)?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|e| AppError::msg(format!("打开安装器失败：{e}")))?;
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| AppError::msg(format!("读取安装器条目失败：{e}")))?;
            let name = entry.name().to_string();
            if !name.starts_with("maven/") || entry.is_dir() {
                continue;
            }
            let rel = name.trim_start_matches("maven/");
            let dest = libs2.join(rel);
            if dest.is_file() {
                continue; // 已存在（增量）
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut entry, &mut out)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("解压任务执行失败：{e}"))??;

    // ---- 5. 下载 processor 依赖库（install_profile.libraries）----
    emit_stage(&ctx, "正在下载 Forge 组件库…");
    let lib_requests = build_lib_requests(
        &profile,
        &libs_dir,
        &ctx.loader_type,
        &ctx.mirrors,
        ctx.official_first,
    );
    download_requests(&ctx, lib_requests).await?;

    // ---- 6. 快速路径：产物已存在且校验通过 → 跳过 processors ----
    let data_client = |key: &str| -> Option<String> {
        profile
            .pointer(&format!("/data/{key}/client"))
            .and_then(|v| v.as_str())
            .map(String::from)
    };
    let patched_raw = data_client("PATCHED")
        .ok_or("install_profile 缺少 PATCHED 数据项")?;
    let patched_rel = coords_to_rel(patched_raw.trim_matches(|c| c == '[' || c == ']'))?;
    let patched_path = libs_dir.join(&patched_rel);
    let expected_sha = data_client("PATCHED_SHA")
        .map(|s| s.trim_matches('\'').to_string());
    let mut already_done = false;
    if let Some(sha) = &expected_sha {
        already_done = crate::download::hash::verify_file(&patched_path, HashAlgo::Sha1, sha)
            .await
            .unwrap_or(false);
    }

    if !already_done {
        // ---- 7. 选择执行 processor 用的 Java（游戏所需版本，缺失自动补装）----
        emit_stage(&ctx, "正在检查 Java 运行时…");
        let java = ensure_java(&ctx).await?;

        // ---- 8. 执行 processors ----
        run_processors(&ctx, &profile, &java, &libs_dir, &work_dir).await?;
    } else {
        emit_stage(&ctx, "已存在有效安装产物，跳过处理器…");
    }

    // ---- 9. 校验 PATCHED 产物并复制为主 jar（版本文件夹 = 实例文件夹）----
    if let Some(sha) = &expected_sha {
        let ok = crate::download::hash::verify_file(&patched_path, HashAlgo::Sha1, sha)
            .await
            .unwrap_or(false);
        if !ok {
            return Err("Forge 安装产物校验失败（patch 后的 jar 哈希不匹配），请重试安装".into());
        }
    }
    let version_dir = ctx.paths.instance_dir(&ctx.instance_id);
    tokio::fs::create_dir_all(&version_dir)
        .await
        .map_err(|e| format!("创建版本目录失败：{e}"))?;
    let main_jar = version_dir.join(format!("{}.jar", ctx.instance_id));
    if patched_path != main_jar {
        tokio::fs::copy(&patched_path, &main_jar)
            .await
            .map_err(|e| format!("复制 patch 后主 jar 失败：{e}"))?;
    }

    // ---- 10. 合并 version.json 并写回（含 client-extra 条目追加）----
    emit_stage(&ctx, "正在写入版本元数据…");
    let mut merged = crate::version::resolve::merge_version_json(&ctx.version_json, &installer_version);
    if let Some(obj) = merged.as_object_mut() {
        obj.remove("inheritsFrom");
        // id = 实例名（版本文件夹名），与主 jar / ${version_name} 保持一致
        obj.insert("id".to_string(), json!(ctx.instance_id));
        // client-extra（原版 jar 拆分产物）是启动 classpath 必需项，
        // installer version.json 不含它，按 MC_EXTRA 坐标手动追加
        if let Some(extra_raw) = data_client("MC_EXTRA") {
            let coords = extra_raw.trim_matches(|c| c == '[' || c == ']');
            if let Ok(rel) = coords_to_rel(coords) {
                if let Some(arr) = obj.get_mut("libraries").and_then(|v| v.as_array_mut()) {
                    let name_key = coords;
                    if !arr
                        .iter()
                        .any(|l| l.get("name").and_then(|v| v.as_str()) == Some(name_key))
                    {
                        arr.push(json!({ "name": name_key }));
                    }
                }
            }
        }
    }
    let v_text = serde_json::to_string_pretty(&merged)
        .map_err(|e| format!("序列化 version.json 失败：{e}"))?;
    write_back_and_finish(&ctx, &v_text).await
}

/// 写回 version.json / loader.json（installed=true）并推送完成事件（两种管线共用）
async fn write_back_and_finish(ctx: &ForgeInstallCtx, v_text: &str) -> Result<(), String> {
    let loader_info = json!({
        "type": ctx.loader_type,
        "version": ctx.full_version,
        "optifineVersion": null,
        "installed": true,
    });
    let l_text = serde_json::to_string_pretty(&loader_info)
        .map_err(|e| format!("序列化 loader.json 失败：{e}"))?;
    let meta_dir = ctx.paths.instance_meta_dir(&ctx.instance_id);
    tokio::fs::create_dir_all(&meta_dir)
        .await
        .map_err(|e| format!("创建实例元数据目录失败：{e}"))?;
    tokio::fs::write(ctx.paths.instance_version_json(&ctx.instance_id), v_text)
        .await
        .map_err(|e| format!("写入 version.json 失败：{e}"))?;
    tokio::fs::write(meta_dir.join("loader.json"), l_text)
        .await
        .map_err(|e| format!("写入 loader.json 失败：{e}"))?;

    emit_stage(ctx, "安装完成");
    if let Some(app) = &ctx.app {
        let _ = app.emit("download-progress", {
            DownloadProgress {
                task_id: ctx.task_id.clone(),
                kind: "loader".into(),
                instance_id: Some(ctx.instance_id.clone()),
                state: "completed".into(),
                current_file: None,
                completed_files: 0,
                total_files: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                bytes_per_second: 0.0,
                error: None,
            }
        });
        let _ = app.emit("instances-changed", ());
    }
    Ok(())
}

/// 旧版 Forge 安装（≤1.12.2，install_profile 无 processors）：
/// launchwrapper 时代无需运行补丁处理器——binpatches 由 FML 的 tweakClass
/// 在游戏运行时解压应用；安装期只需：
///   解压内置库 → 下载组件库（老格式 maven 坐标 + 基址 URL）→
///   复制原版 client.jar 作为主 jar → 合并 versionInfo 写回。
async fn install_legacy_forge(
    ctx: &ForgeInstallCtx,
    profile: &Value,
    version_info: &Value,
    libs_dir: &Path,
) -> Result<(), String> {
    let installer_jar = ctx
        .paths
        .cache_dir()
        .join("forge")
        .join(&ctx.full_version)
        .join(format!("{}-installer.jar", ctx.full_version));

    // ---- 1. 解压 installer 内置 maven/ 库（forge universal 等）----
    emit_stage(ctx, "正在解压安装器内置库…");
    let maven_jar = installer_jar.clone();
    let libs2 = libs_dir.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let file = std::fs::File::open(&maven_jar)?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| AppError::msg(format!("打开安装器失败：{e}")))?;
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| AppError::msg(format!("读取安装器条目失败：{e}")))?;
            let name = entry.name().to_string();
            if !name.starts_with("maven/") || entry.is_dir() {
                continue;
            }
            let rel = name.trim_start_matches("maven/");
            let dest = libs2.join(rel);
            if dest.is_file() {
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| AppError::msg(format!("创建库目录失败：{e}")))?;
            }
            let mut out = std::fs::File::create(&dest)
                .map_err(|e| AppError::msg(format!("创建库文件失败：{e}")))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| AppError::msg(format!("解压库文件失败：{e}")))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("解压任务执行失败：{e}"))??;

    // ---- 2. 下载组件库（老格式：name + url 基址 + checksums；clientreq 过滤）----
    emit_stage(ctx, "正在下载 Forge 组件库…");
    let requests = build_legacy_lib_requests(
        profile.get("libraries"),
        version_info.get("libraries"),
        libs_dir,
        &ctx.mirrors,
        ctx.official_first,
    );
    download_requests(ctx, requests).await?;

    // ---- 3. 主 jar 就位校验：新布局下原版 client.jar 已由资源补全下载到
    //         版本文件夹（{实例名}.jar），launchwrapper 所需的主 jar 即它本身，
    //         binpatch 在运行时由 FML 应用，安装期无需复制 ----
    let main_jar = ctx
        .paths
        .instance_dir(&ctx.instance_id)
        .join(format!("{}.jar", ctx.instance_id));
    if !main_jar.is_file() {
        return Err(format!("原版主 jar 缺失：{}", main_jar.display()));
    }

    // ---- 4. 合并 versionInfo 写回 ----
    emit_stage(ctx, "正在写入版本元数据…");
    let mut merged = crate::version::resolve::merge_version_json(&ctx.version_json, version_info);
    if let Some(obj) = merged.as_object_mut() {
        obj.remove("inheritsFrom");
        // id = 实例名（版本文件夹名），与主 jar / ${version_name} 保持一致
        obj.insert("id".to_string(), json!(ctx.instance_id));
    }
    let v_text = serde_json::to_string_pretty(&merged)
        .map_err(|e| format!("序列化 version.json 失败：{e}"))?;
    write_back_and_finish(ctx, &v_text).await
}

/// 老格式库条目 → 下载请求（versionInfo.libraries + install_profile.libraries 合并去重）
/// 条目结构：{ name, url?, checksums?: [sha1], clientreq?, serverreq? }
fn build_legacy_lib_requests(
    profile_libs: Option<&Value>,
    version_libs: Option<&Value>,
    libs_dir: &Path,
    mirrors: &[String],
    official_first: bool,
) -> Vec<DownloadRequest> {
    let mut out: Vec<DownloadRequest> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for group in [profile_libs, version_libs].into_iter().flatten() {
        let Some(arr) = group.as_array() else { continue };
        for lib in arr {
            let Some(name) = lib.get("name").and_then(|v| v.as_str()) else { continue };
            // clientreq = false 的库是服务端专用，客户端不下载
            if lib.get("clientreq").and_then(|v| v.as_bool()) == Some(false) {
                continue;
            }
            // natives-only 库（jinput-platform / lwjgl-platform 等）：官方仅托管各平台
            // classifier jar，没有无后缀主 jar（实测 404）——主 jar 跳过，natives 部分
            // 由原版资源补全（plan_base 按 classifiers）下载并解压
            let has_natives = lib
                .get("natives")
                .and_then(|v| v.as_object())
                .map(|m| !m.is_empty())
                .unwrap_or(false);
            let has_artifact = lib.pointer("/downloads/artifact").is_some();
            if has_natives && !has_artifact {
                continue;
            }
            let Some(rel) = planner::maven_path_from_name(name) else { continue };
            let dest = libs_dir.join(&rel);
            if !seen.insert(dest.clone()) || dest.is_file() {
                continue;
            }
            // 官方直链：优先 downloads.artifact.url（新格式，version.json 的库条目
            // 直接指向真实源，如 maven.minecraftforge.net）；缺失时回落老格式
            // url 基址字段（1.6-1.12 早期 install_profile），再缺省 Mojang 库。
            // files.minecraftforge.net 已改为 301 跳转（且 Fastly 国内边缘偶发
            // 异常 404），重写为 maven 域真源减少一跳。
            let artifact_url = lib
                .pointer("/downloads/artifact/url")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    s.trim_end_matches('/')
                        .replace("http://", "https://")
                        .replace(
                            "https://files.minecraftforge.net",
                            "https://maven.minecraftforge.net",
                        )
                });
            let official_url = match artifact_url {
                Some(u) => u,
                None => {
                    let base = lib
                        .get("url")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("https://libraries.minecraft.net/")
                        .trim_end_matches('/')
                        .replace("http://", "https://")
                        .replace(
                            "https://files.minecraftforge.net",
                            "https://maven.minecraftforge.net",
                        );
                    format!("{base}/{rel}")
                }
            };
            let mut candidates: Vec<String> = Vec::new();
            let is_forge_maven = official_url.contains("maven.minecraftforge.net")
                || official_url.contains("files.minecraftforge.net");
            // BMCLAPI 镜像（仅 Forge maven）：直接用 maven 相对路径拼 BMCLAPI 根
            if is_forge_maven {
                let path_part = rel.clone();
                let bmcl = format!("{BMCLAPI_MAVEN}/{path_part}");
                if official_first {
                    candidates.push(official_url.clone());
                    candidates.push(bmcl);
                } else {
                    candidates.push(bmcl);
                    candidates.push(official_url.clone());
                }
            } else {
                candidates.push(official_url);
            }
            // Maven Central 兜底：Forge/老版 installer 引用的部分第三方库
            //（如 asm-debug-all）不在 Forge maven / Mojang 库上（实测 404），
            // 一定在 Central 上，追加为最后候选
            let central = format!("https://repo1.maven.org/maven2/{rel}");
            if !candidates.contains(&central) {
                candidates.push(central);
            }
            // 用户自定义镜像兜底（仅 Forge maven）
            for m in mirrors {
                if is_forge_maven {
                    let cand = format!("{}/{}", m.trim_end_matches('/'), rel);
                    if !candidates.contains(&cand) {
                        candidates.push(cand);
                    }
                }
            }
            let mut req = DownloadRequest::new(format!("libraries/{rel}"), candidates, dest);
            if let Some(sums) = lib.get("checksums").and_then(|v| v.as_array()) {
                if let Some(sha) = sums.first().and_then(|v| v.as_str()) {
                    req.expected = Some((HashAlgo::Sha1, sha.to_lowercase()));
                }
            }
            out.push(req);
        }
    }
    out
}

/// 原版资源补全（增量：缺失才下载；与启动前检查同逻辑，oneshot 等待完成）
async fn ensure_base_resources(ctx: &ForgeInstallCtx) -> Result<(), String> {
    let v = &ctx.version_json;
    let game_root = &ctx.game_root;
    let index_id = v
        .pointer("/assetIndex/id")
        .and_then(|x| x.as_str())
        .unwrap_or("legacy")
        .to_string();
    let index_path = game_root
        .join("assets")
        .join("indexes")
        .join(format!("{index_id}.json"));
    if !index_path.exists() {
        planner::ensure_asset_index(v, &index_path, &ctx.mirrors, ctx.official_first)
            .await
            .map_err(String::from)?;
    }
    let mut requests = planner::plan_base(v, game_root, &ctx.mirrors, ctx.official_first)
        .map_err(String::from)?;
    let objects = planner::plan_objects(&index_path, game_root, &ctx.mirrors, ctx.official_first)
        .await
        .map_err(String::from)?;
    requests.extend(objects);
    // 引擎内置增量跳过，直接交给下载任务
    download_requests(ctx, requests).await
}

/// 由 install_profile.libraries 构造下载请求
/// （库条目均带 downloads.artifact；url 为空时回退对应官方 maven + BMCLAPI 镜像）
fn build_lib_requests(
    profile: &Value,
    libs_dir: &Path,
    loader_type: &str,
    mirrors: &[String],
    official_first: bool,
) -> Vec<DownloadRequest> {
    let mut out: Vec<DownloadRequest> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let Some(libs) = profile.get("libraries").and_then(|v| v.as_array()) else {
        return out;
    };
    for lib in libs {
        let Some(name) = lib.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        // 同一目标路径去重（install_profile.libraries 可能含重复条目）
        let dedup_key = match lib.pointer("/downloads/artifact/path").and_then(|v| v.as_str()) {
            Some(p) => PathBuf::from(p),
            None => match planner::maven_path_from_name(name) {
                Some(p) => PathBuf::from(p),
                None => continue,
            },
        };
        if !seen.insert(dedup_key) {
            continue;
        }
        // sides 过滤：processor 依赖库不做 sided 过滤（server 专用库也会被 client 下载，
        // 体量小，保持简单）
        let Some(art) = lib.pointer("/downloads/artifact") else {
            continue;
        };
        let path = art.get("path").and_then(|v| v.as_str()).map(String::from);
        let url = art.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let sha = art.get("sha1").and_then(|v| v.as_str()).map(String::from);
        let size = art.get("size").and_then(|v| v.as_u64());
        let rel = match path {
            Some(p) => p,
            None => match planner::maven_path_from_name(name) {
                Some(p) => p,
                None => continue,
            },
        };
        // URL 候选：镜像优先 / 官方兜底（官方 maven 按加载器类型选择；库无主 URL 时补默认仓库）
        let mut candidates: Vec<String> = Vec::new();
        let official_base = match loader_type {
            "neoforge" => NEOFORGE_MAVEN,
            _ => FORGE_MAVEN,
        };
        let official_url = if url.is_empty() {
            format!("{official_base}/{rel}")
        } else {
            url.to_string()
        };
        // BMCLAPI 只镜像 Forge maven；libraries.minecraft.net 由 mirror 规则处理
        if official_url.contains("maven.minecraftforge.net") {
            let bmcl = official_url.replace(FORGE_MAVEN, BMCLAPI_MAVEN);
            if official_first {
                candidates.push(official_url.clone());
                candidates.push(bmcl);
            } else {
                candidates.push(bmcl);
                candidates.push(official_url.clone());
            }
        } else if official_url.contains("maven.neoforged.net") {
            // NeoForge 官方 maven 国内不稳定，加 BMCLAPI 镜像候选（/maven/ 同路径）
            let bmcl = official_url.replace(NEOFORGE_MAVEN, BMCLAPI_MAVEN);
            if official_first {
                candidates.push(official_url.clone());
                candidates.push(bmcl);
            } else {
                candidates.push(bmcl);
                candidates.push(official_url.clone());
            }
        } else {
            candidates.push(official_url.clone());
        }
        // Maven Central 兜底（第三方库可能不在 Forge/NeoForge maven 上）
        let central = format!("https://repo1.maven.org/maven2/{rel}");
        if !candidates.contains(&central) {
            candidates.push(central);
        }
        // 用户自定义镜像兜底追加（仅对 Forge maven 生效）
        for m in mirrors {
            let cand = official_url.replace(FORGE_MAVEN, m.trim_end_matches('/'));
            if !candidates.contains(&cand) {
                candidates.push(cand);
            }
        }
        let dest = libs_dir.join(&rel);
        if dest.is_file() {
            continue; // 内置解压已覆盖，跳过
        }
        let mut req = DownloadRequest::new(format!("libraries/{rel}"), candidates, dest);
        if let Some(sha) = sha {
            req.expected = Some((HashAlgo::Sha1, sha));
        }
        req.size = size;
        out.push(req);
    }
    out
}

/// 选择执行 processor 的 Java（游戏所需主版本；缺失走自动补装流程）
async fn ensure_java(ctx: &ForgeInstallCtx) -> Result<PathBuf, String> {
    use crate::java::detect;
    let required = detect::required_java(&ctx.mc_version);
    let paths = ctx.paths.clone();
    let p2 = paths.clone();
    let javas = tauri::async_runtime::spawn_blocking(move || detect::detect_all(&p2))
        .await
        .map_err(|e| format!("检测任务执行失败：{e}"))?;
    if let Some(j) =
        crate::launch::args::select_java(&javas, required, detect::max_java(&ctx.mc_version))
    {
        return Ok(PathBuf::from(&j.path));
    }
    // 自动补装（与启动流程同一入口）；测试环境（app=None）不支持补装
    let app = ctx
        .app
        .as_ref()
        .ok_or_else(|| format!("Java {required} 未检测到，无法运行安装器处理器（测试环境无自动补装）"))?;
    crate::commands::java::install_java(
        app,
        &ctx.download,
        ctx.active.clone(),
        ctx.retry_defs.clone(),
        &paths,
        required,
    )
    .await
    .map_err(String::from)?;
    let p3 = paths.clone();
    let javas2 = tauri::async_runtime::spawn_blocking(move || detect::detect_all(&p3))
        .await
        .map_err(|e| format!("检测任务执行失败：{e}"))?;
    crate::launch::args::select_java(&javas2, required, detect::max_java(&ctx.mc_version))
        .map(|j| PathBuf::from(&j.path))
        .ok_or_else(|| format!("Java {required} 已安装但未被检测到，请重启启动器后重试"))
}

/// 执行 install_profile 中的 processors（client 侧）
async fn run_processors(
    ctx: &ForgeInstallCtx,
    profile: &Value,
    java: &Path,
    libs_dir: &Path,
    work_dir: &Path,
) -> Result<(), String> {
    let processors = profile
        .get("processors")
        .and_then(|v| v.as_array())
        .ok_or("install_profile 缺少 processors")?;
    let data = profile
        .get("data")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    // 预置环境变量
    let mc_jar = ctx
        .paths
        .instance_dir(&ctx.instance_id)
        .join(format!("{}.jar", ctx.instance_id));
    let mut env: HashMap<String, String> = HashMap::new();
    env.insert("MINECRAFT_JAR".into(), mc_jar.to_string_lossy().into());
    env.insert("SIDE".into(), "client".into());
    env.insert("ROOT".into(), ctx.game_root.to_string_lossy().into());
    env.insert("LIBRARY_DIR".into(), libs_dir.to_string_lossy().into());
    // INSTALLER 在运行时填充（闭包里持 zip 不可行 → 预先把 data 引用的
    // installer 内文件解出，再以解出路径参与替换）

    // 先把 data 中所有 client 侧值解析为最终字符串
    let installer_jar = work_dir.join(format!("{}-installer.jar", ctx.full_version));
    let extract_root = work_dir.join("data");
    let mut resolved_data: HashMap<String, String> = HashMap::new();
    {
        let mut zip = zip::ZipArchive::new(
            std::fs::File::open(&installer_jar)
                .map_err(|e| format!("打开安装器失败：{e}"))?,
        )
        .map_err(|e| format!("打开安装器失败：{e}"))?;
        for (key, val) in &data {
            let raw = val
                .get("client")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let r = resolve_value(
                raw,
                &env,
                &resolved_data,
                &mut zip,
                &extract_root,
                libs_dir,
                0,
            )
            .map_err(|e| format!("解析数据项 {key} 失败：{e}"))?;
            resolved_data.insert(key.clone(), r);
        }
    }
    env.insert("INSTALLER".into(), installer_jar.to_string_lossy().into());

    let total = processors.len();
    for (i, p) in processors.iter().enumerate() {
        // sides 过滤：仅执行 client / 未标注 side 的处理器
        if let Some(sides) = p.get("sides").and_then(|v| v.as_array()) {
            let ok = sides
                .iter()
                .filter_map(|v| v.as_str())
                .any(|s| s == "client");
            if !ok {
                continue;
            }
        }
        let jar = p
            .get("jar")
            .and_then(|v| v.as_str())
            .ok_or("processor 缺少 jar 字段")?;
        emit_stage(
            ctx,
            &format!("正在执行安装处理器（{}/{}）：{}", i + 1, total, jar),
        );

        // classpath：processor jar 自身 + 依赖列表
        let mut cp: Vec<String> = vec![{
            let rel = planner::maven_path_from_name(jar)
                .ok_or_else(|| format!("无法解析 processor 坐标：{jar}"))?;
            libs_dir.join(rel).to_string_lossy().into()
        }];
        if let Some(deps) = p.get("classpath").and_then(|v| v.as_array()) {
            for d in deps {
                let coord = d.as_str().ok_or("processor classpath 含非字符串项")?;
                let rel = coords_to_rel(coord)?;
                cp.push(libs_dir.join(rel).to_string_lossy().into());
            }
        }

        // 主类：profile main 字段优先，回落 jar 的 MANIFEST Main-Class
        let main_class = match p.get("main").and_then(|v| v.as_str()) {
            Some(m) => m.to_string(),
            None => {
                let jar_path = cp[0].clone();
                let jar_path2 = jar_path.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    read_manifest_main(Path::new(&jar_path2))
                })
                    .await
                    .map_err(|e| format!("读取任务失败：{e}"))?
                    .ok_or_else(|| format!("processor {jar} 缺少主类（无 main 字段且 MANIFEST 无 Main-Class）"))?
            }
        };

        // 参数替换
        let raw_args = p
            .get("args")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut args: Vec<String> = Vec::with_capacity(raw_args.len());
        {
            let mut zip = zip::ZipArchive::new(
                std::fs::File::open(&installer_jar)
                    .map_err(|e| format!("打开安装器失败：{e}"))?,
            )
            .map_err(|e| format!("打开安装器失败：{e}"))?;
            for a in &raw_args {
                let raw = a.as_str().ok_or("processor args 含非字符串项")?;
                let r = resolve_value(raw, &env, &resolved_data, &mut zip, &extract_root, libs_dir, 0)
                    .map_err(|e| format!("替换参数失败：{e}"))?;
                args.push(r);
            }
        }
        // jvm 参数（存在时一并替换；1.20.1 无此字段）
        let mut jvm_args: Vec<String> = Vec::new();
        if let Some(jv) = p.get("jvm").and_then(|v| v.as_array()) {
            let mut zip = zip::ZipArchive::new(
                std::fs::File::open(&installer_jar)
                    .map_err(|e| format!("打开安装器失败：{e}"))?,
            )
            .map_err(|e| format!("打开安装器失败：{e}"))?;
            for a in jv {
                let raw = a.as_str().ok_or("processor jvm 含非字符串项")?;
                let r = resolve_value(raw, &env, &resolved_data, &mut zip, &extract_root, libs_dir, 0)
                    .map_err(|e| format!("替换 JVM 参数失败：{e}"))?;
                jvm_args.push(r);
            }
        }

        // 执行（CREATE_NO_WINDOW 防黑窗）
        let mut cmd = tokio::process::Command::new(java);
        cmd.args(&jvm_args)
            .arg("-cp")
            .arg(cp.join(";"))
            .arg(&main_class)
            .args(&args)
            .current_dir(work_dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let output = cmd
            .output()
            .await
            .map_err(|e| format!("启动 processor 失败：{e}"))?;
        if !output.status.success() {
            let tail: String = String::from_utf8_lossy(&output.stderr)
                .lines()
                .rev()
                .take(15)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!(
                "安装处理器 {jar} 执行失败（退出码 {:?}）：\n{tail}",
                output.status.code()
            ));
        }
    }
    Ok(())
}

/// 解析 install_profile 的 data/参数值：
/// [maven坐标] / {KEY} / /data/xxx（installer 内文件，解出后返回路径）/ 'sha' 字面量 / 混合串
fn resolve_value(
    raw: &str,
    env: &HashMap<String, String>,
    data: &HashMap<String, String>,
    zip: &mut zip::ZipArchive<std::fs::File>,
    extract_root: &Path,
    libs_dir: &Path,
    depth: u32,
) -> AppResult<String> {
    if depth > 8 {
        return Err(AppError::msg("数据项引用层级过深（疑似循环引用）"));
    }
    let t = raw.trim();
    // [maven 坐标] → 库绝对路径
    if t.starts_with('[') && t.ends_with(']') && t.len() > 2 {
        let rel = coords_to_rel(&t[1..t.len() - 1])?;
        return Ok(libs_dir.join(rel).to_string_lossy().into());
    }
    // /data/xxx → 从 installer 解出
    if let Some(rel) = t.strip_prefix("/data/") {
        let dest = extract_root.join(rel);
        if !dest.is_file() {
            let parent = dest
                .parent()
                .ok_or_else(|| AppError::msg("data 文件路径异常"))?;
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::msg(format!("创建数据目录失败：{e}")))?;
            let mut f = zip
                .by_name(&format!("data/{rel}"))
                .map_err(|e| AppError::msg(format!("installer 内缺少 {rel}：{e}")))?;
            let mut out = std::fs::File::create(&dest)
                .map_err(|e| AppError::msg(format!("创建数据文件失败：{e}")))?;
            std::io::copy(&mut f, &mut out)
                .map_err(|e| AppError::msg(format!("解出数据文件失败：{e}")))?;
        }
        return Ok(dest.to_string_lossy().into());
    }
    // {KEY} 全串引用
    if t.starts_with('{') && t.ends_with('}') && t.len() > 2 {
        let key = &t[1..t.len() - 1];
        if let Some(v) = env.get(key) {
            return Ok(v.clone());
        }
        if let Some(v) = data.get(key) {
            return Ok(v.clone());
        }
        return Err(AppError::msg(format!("未知数据项：{key}")));
    }
    // 'xxx' 带引号字面量（官方用作哈希校验标记，替换时去引号）
    if t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2 {
        return Ok(t[1..t.len() - 1].to_string());
    }
    // 混合串：内嵌 {X} 占位（如 --archive {INSTALLER} 之外的组合形式）
    if t.contains('{') {
        let mut result = String::with_capacity(t.len());
        let bytes: Vec<char> = t.chars().collect();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '{' {
                if let Some(end) = bytes[i..].iter().position(|c| *c == '}') {
                    let key: String = bytes[i + 1..i + end].iter().collect();
                    let val = resolve_value(
                        &format!("{{{key}}}"),
                        env,
                        data,
                        zip,
                        extract_root,
                        libs_dir,
                        depth + 1,
                    )?;
                    result.push_str(&val);
                    i += end + 1;
                    continue;
                }
            }
            result.push(bytes[i]);
            i += 1;
        }
        return Ok(result);
    }
    Ok(t.to_string())
}

/// 从 jar 的 MANIFEST.MF 读取 Main-Class
fn read_manifest_main(jar: &Path) -> Option<String> {
    let file = std::fs::File::open(jar).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut mf = zip.by_name("META-INF/MANIFEST.MF").ok()?;
    let mut text = String::new();
    mf.read_to_string(&mut text).ok()?;
    // MANIFEST 行折叠规则：续行以单个空格开头
    let mut unfolded = String::new();
    for line in text.lines() {
        if line.starts_with(' ') {
            unfolded.push_str(line.trim_start());
        } else {
            if !unfolded.is_empty() {
                unfolded.push('\n');
            }
            unfolded.push_str(line);
        }
    }
    unfolded.lines().find_map(|l| {
        l.strip_prefix("Main-Class:")
            .map(|v| v.trim().trim_end_matches('.').to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// natives-only 库（jinput-platform 等）不构造主 jar 下载（官方源 404，实测确认）
    #[test]
    fn legacy_lib_requests_skip_natives_only() {
        use serde_json::json;
        let libs = json!([
            // natives-only：无 downloads.artifact，应跳过
            { "name": "net.java.jinput:jinput-platform:2.0.5",
              "natives": { "linux": "natives-linux", "windows": "natives-windows", "osx": "natives-osx" } },
            // 主 jar 库（有官方 artifact），应保留且指向 Mojang 库
            { "name": "net.java.jinput:jinput:2.0.5" },
            // 服务端专用库，clientreq=false 应跳过
            { "name": "net.minecraftforge:forge:1.7.10-10.13.0.1151", "clientreq": false }
        ]);
        let reqs = build_legacy_lib_requests(
            Some(&libs),
            None,
            Path::new("C:/mc/libraries"),
            &[],
            true,
        );
        let keys: Vec<&str> = reqs.iter().map(|r| r.key.as_str()).collect();
        assert!(
            !keys.iter().any(|k| k.contains("jinput-platform")),
            "natives-only 库不应出现在下载列表：{keys:?}"
        );
        assert!(
            keys.iter().any(|k| k.contains("jinput-2.0.5.jar")),
            "jinput 主 jar 应在下载列表：{keys:?}"
        );
        let jinput = reqs.iter().find(|r| r.key.contains("jinput-2.0.5.jar")).unwrap();
        assert!(
            jinput.urls[0].starts_with("https://libraries.minecraft.net/"),
            "无 url 条目应回落 Mojang 库（且为 https）：{:?}",
            jinput.urls
        );
    }
}

