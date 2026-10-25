// 启动流程命令：launch_instance（状态机事件驱动）/ kill_instance / list_running
//
// launch_instance 立即返回，后台任务按阶段推进并推送 launch-state 事件：
//   checking_java → checking_resources → [downloading（进度走 download-progress）]
//   → extracting_natives → starting → running / failed
// 进程退出时推送 launch-exited { code, summary }，latest.log 落盘到实例 logs 目录。
// kill_instance 通过 TerminateProcess 强杀，收尾统一由退出监视任务完成。

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::RwLock;

use crate::commands::AppState;
use crate::core::paths::LauncherPaths;
use crate::core::settings::AppSettings;
use crate::download::{DownloadEngine, ProgressCallback, TaskOptions};
use crate::instance::model::LoaderInfo;
use crate::instance::store::InstanceStore;
use crate::java::detect;
use crate::java::model::JavaInfo;
use crate::launch::{args, natives, process};
use crate::game::planner;
use crate::version::repo::VersionRepo;

/// 启动依赖包（可整体 move 进后台任务）
#[derive(Clone)]
struct LaunchDeps {
    app: AppHandle,
    paths: LauncherPaths,
    instances: InstanceStore,
    versions: VersionRepo,
    download: DownloadEngine,
    active_downloads: Arc<StdMutex<HashMap<String, String>>>,
    retry_defs: Arc<StdMutex<HashMap<String, super::RetryDef>>>,
    settings: Arc<RwLock<AppSettings>>,
    running: Arc<StdMutex<HashMap<String, Arc<process::RunningProcess>>>>,
    killed: Arc<StdMutex<HashSet<String>>>,
}

/// 发射 launch-state 事件
fn emit_state(app: &AppHandle, instance_id: &str, stage: &str, message: &str) {
    let _ = app.emit(
        "launch-state",
        json!({ "instanceId": instance_id, "stage": stage, "message": message }),
    );
}

/// 发起启动：立即返回，进度与结果经 launch-state / launch-log / launch-exited 事件推送
/// server_address：可选的启动后直连服务器地址（host / host:port，支持 SRV）
#[tauri::command]
pub async fn launch_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    server_address: Option<String>,
) -> Result<(), String> {
    if !state.paths.instance_dir(&instance_id).is_dir() {
        return Err(format!("实例 {instance_id} 不存在"));
    }
    if state.running.lock().unwrap().contains_key(&instance_id) {
        return Err("该实例已在运行中".to_string());
    }
    // 传入直连地址时先校验格式（SRV 解析在后台任务内执行）
    if let Some(addr) = &server_address {
        crate::servers::store::validate_address(addr)?;
    }
    let deps = LaunchDeps {
        app,
        paths: state.paths.clone(),
        instances: state.instances.clone(),
        versions: state.versions.clone(),
        download: state.download.clone(),
        active_downloads: state.active_downloads.clone(),
        retry_defs: state.retry_defs.clone(),
        settings: state.settings.clone(),
        running: state.running.clone(),
        killed: state.killed.clone(),
    };
    tokio::spawn(async move {
        if let Err(e) = run_launch(&deps, &instance_id, server_address).await {
            emit_state(&deps.app, &instance_id, "failed", &e);
        }
    });
    Ok(())
}

/// 启动状态机主流程
async fn run_launch(
    deps: &LaunchDeps,
    instance_id: &str,
    server_address: Option<String>,
) -> Result<(), String> {
    let app = &deps.app;
    let emit = |stage: &str, message: &str| emit_state(app, instance_id, stage, message);

    // ---- 1. 读取实例与版本元数据（Java 需求与资源检查都依赖 version.json）----
    emit("checking_java", "正在检查 Java…");
    let detail = deps.instances.get(instance_id).await.map_err(String::from)?;
    let mc_version = detail.summary.mc_version.clone();
    // 版本元数据：优先 .pie\version.json（外部实例导入时的冻结继承链合并版，
    // 原启动器的 {id}.json 未被改动、其 inheritsFrom 不再存在于冻结版中）；
    // 自家实例无该文件，回落 {id}.json
    let frozen_path = deps
        .paths
        .instance_meta_dir(instance_id)
        .join("version.json");
    let version_text = match tokio::fs::read_to_string(&frozen_path).await {
        Ok(t) => t,
        Err(_) => tokio::fs::read_to_string(deps.paths.instance_version_json(instance_id))
            .await
            .map_err(|_| "实例缺少版本元数据（version json）".to_string())?,
    };
    let mut version_json: Value = serde_json::from_str(&version_text)
        .map_err(|e| format!("version.json 解析失败：{e}"))?;

    // ---- 2. Java 检查与选择 ----
    // 官方标注优先：javaVersion.majorVersion 随版本发布，能跟上未来新版本；
    // 极老版本（无该字段）回落 detect::required_java 规则表
    let required = detect::required_java_from_json(&version_json)
        .unwrap_or_else(|| detect::required_java(&mc_version));
    let scan_paths = deps.paths.clone();
    let javas = tauri::async_runtime::spawn_blocking(move || detect::detect_all(&scan_paths))
        .await
        .map_err(|e| format!("检测任务执行失败：{e}"))?;
    let settings = deps.settings.read().await.clone();

    // ---- 2.5 账户：激活账户决定认证参数（全局作用域）----
    let account_root = deps.paths.root().to_path_buf();
    let account_store = tauri::async_runtime::spawn_blocking(move || {
        crate::account::store::AccountStore::load(&account_root)
    })
    .await
    .map_err(|e| format!("账户读取任务失败：{e}"))?;
    let account = account_store
        .active()
        .ok_or_else(|| "尚未选择账户，请先到「账户管理」页添加并选择一个账户".to_string())?;
    let account_ref = account;
    let auth = if account_ref.is_offline() {
        args::AuthContext::offline(&account_ref.name)
    } else {
        // 校验/刷新微软 token 后返回真实认证参数（刷新会写回 accounts.json）
        let root = deps.paths.root().to_path_buf();
        crate::account::ms_auth::ensure_mc_auth(account_ref, &root).await?
    };
    let player_name = account_ref.name.clone();

    // ---- 2.6 离线皮肤注入（批次 D）：起本地会话服务 + 准备 javaagent 参数 ----
    let mut extra_jvm: Vec<String> = Vec::new();
    let mut ygg_handle: Option<crate::account::local_ygg::LocalYggHandle> = None;
    if account_ref.is_offline() {
        if let Some(skin_meta) = &account_ref.offline_skin {
            emit("checking_java", "正在准备离线皮肤注入…");
            let jar =
                crate::account::local_ygg::ensure_agent_jar(&deps.paths.cache_dir()).await?;
            let png = crate::account::skin::read_skin_png(&deps.paths.root(), &account_ref.id)
                .ok_or("皮肤文件丢失，请到「账户管理」页重新抓取")?;
            let handle = crate::account::local_ygg::start(
                account_ref.uuid.replace('-', ""),
                account_ref.name.clone(),
                skin_meta.variant.clone(),
                png,
            )
            .await?;
            eprintln!("[local-ygg] 注入 javaagent（端口 {}）", handle.port);
            extra_jvm.push(format!(
                "-javaagent:{}=http://127.0.0.1:{}",
                jar.display(),
                handle.port
            ));
            ygg_handle = Some(handle);
        }
    }

    let java: JavaInfo = match &detail.java_path {
        Some(p) => {
            let found = javas
                .iter()
                .find(|j| j.path.eq_ignore_ascii_case(p))
                .ok_or_else(|| {
                    "指定的 Java 路径不可用或无法识别版本，请在实例设置中重新选择".to_string()
                })?;
            let max = detect::max_java(&mc_version);
            if found.version < required {
                return Err(format!(
                    "指定的 Java {} 低于该版本要求的 Java {required}+，请在实例设置中更换",
                    found.version
                ));
            }
            if let Some(m) = max {
                if found.version > m {
                    return Err(format!(
                        "指定的 Java {} 高于该版本允许的上限（Java {m} 及更早），\
                         旧版本 Minecraft 与新 Java 不兼容，请在实例设置中更换",
                        found.version
                    ));
                }
            }
            found.clone()
        }
        None => {
            let max = detect::max_java(&mc_version);
            let hint = match max {
                Some(m) => format!("Java {required}–{m}（该版本仅兼容此范围内的 Java）"),
                None => format!("Java {required}"),
            };
            match args::select_java(&javas, required, max) {
                Some(j) => j.clone(),
                None => {
                    // 自动补装缺失的 Java（Adoptium，TUNA 镜像优先），装完重新检测再选择
                    // 列出已检测到的版本，说明为什么需要下载（消除"有 Java 还下载"的困惑）
                    let found = if javas.is_empty() {
                        "未检测到本机有任何 Java".to_string()
                    } else {
                        let mut vs: Vec<String> =
                            javas.iter().map(|j| j.version.to_string()).collect();
                        vs.sort();
                        vs.dedup();
                        format!("已检测到本机 Java {}，但该版本需要 {}", vs.join(" / "), hint)
                    };
                    emit(
                        "downloading",
                        &format!("{found}，正在自动下载安装…"),
                    );
                    crate::commands::java::install_java(
                        &deps.app,
                        &deps.download,
                        deps.active_downloads.clone(),
                        deps.retry_defs.clone(),
                        &deps.paths,
                        required,
                    )
                    .await
                    .map_err(String::from)?;
                    let rescan = deps.paths.clone();
                    let javas2 =
                        tauri::async_runtime::spawn_blocking(move || detect::detect_all(&rescan))
                            .await
                            .map_err(|e| format!("检测任务执行失败：{e}"))?;
                    args::select_java(&javas2, required, max)
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "Java {required} 已安装但未被检测到，请重启启动器后重试"
                            )
                        })?
                }
            }
        }
    };

    // ---- 2.8 加载器补装：骨架实例（loader.json installed=false）启动前自动安装 ----
    // Fabric / Quilt：拉取 profile 合并进冻结 version.json，其 libraries 随下方
    // 资源检查一并下载；写回后下次启动即为已安装状态。Forge / NeoForge 管线
    // 留待后续阶段，给出明确提示。
    let loader_path = deps.paths.instance_meta_dir(instance_id).join("loader.json");
    if let Ok(loader_text) = tokio::fs::read_to_string(&loader_path).await {
        if let Ok(loader) = serde_json::from_str::<LoaderInfo>(&loader_text) {
            if !loader.installed && loader.loader_type != "vanilla" {
                match loader.loader_type.as_str() {
                    "fabric" | "quilt" => {
                        let lv = loader
                            .version
                            .clone()
                            .filter(|v| !v.trim().is_empty())
                            .ok_or("加载器缺少版本信息，请删除实例后重新创建")?;
                        let display = if loader.loader_type == "fabric" { "Fabric" } else { "Quilt" };
                        emit(
                            "checking_resources",
                            &format!("正在安装加载器 {display} {lv}…"),
                        );
                        let profile = deps
                            .versions
                            .fetch_loader_profile(&loader.loader_type, &mc_version, &lv)
                            .await
                            .map_err(String::from)?;
                        let mut merged =
                            crate::version::resolve::merge_version_json(&version_json, &profile);
                        if let Some(obj) = merged.as_object_mut() {
                            obj.remove("inheritsFrom");
                            // id = 实例名（版本文件夹名），与 jar / ${version_name} 保持一致
                            obj.insert("id".to_string(), serde_json::json!(instance_id));
                        }
                        let installed = LoaderInfo {
                            loader_type: loader.loader_type.clone(),
                            version: Some(lv),
                            optifine_version: loader.optifine_version.clone(),
                            installed: true,
                        };
                        let v_path = deps.paths.instance_version_json(instance_id);
                        let v_text = serde_json::to_string_pretty(&merged)
                            .map_err(|e| format!("序列化 version.json 失败：{e}"))?;
                        let l_text = serde_json::to_string_pretty(
                            &serde_json::to_value(&installed).unwrap_or_default(),
                        )
                        .map_err(|e| format!("序列化 loader.json 失败：{e}"))?;
                        tokio::fs::write(&v_path, v_text)
                            .await
                            .map_err(|e| format!("写入 version.json 失败：{e}"))?;
                        tokio::fs::write(deps.paths.instance_meta_dir(instance_id).join("loader.json"), l_text)
                            .await
                            .map_err(|e| format!("写入 loader.json 失败：{e}"))?;
                        version_json = merged;
                        let _ = deps.app.emit("instances-changed", ());
                    }
                    other => {
                        return Err(format!(
                            "该实例的 {other} 加载器尚未安装；Forge / NeoForge 安装将在后续阶段支持"
                        ));
                    }
                }
            }
        }
    }

    // ---- 3. 资源完整性检查（缺失自动补全）----
    emit("checking_resources", "正在检查资源完整性…");
    // 游戏公共根（共享 assets/libraries/versions）与游戏工作目录（按隔离开关派生）
    let game_root = deps.paths.game_root().to_path_buf();
    let game_dir = deps
        .paths
        .instance_game_dir(instance_id, detail.isolation);
    let mirrors = settings.mirrors.clone();
    // 下载源策略：official → 空镜像表（仅官方）；mirror → 镜像在前；auto → 官方在前
    let official_first = settings.download_source != "mirror";
    let effective_mirrors = if settings.download_source == "official" {
        Vec::new()
    } else {
        mirrors.clone()
    };

    let index_id = version_json
        .pointer("/assetIndex/id")
        .and_then(|v| v.as_str())
        .unwrap_or("legacy")
        .to_string();
    let index_path = game_root.join("assets").join("indexes").join(format!("{index_id}.json"));
    if !index_path.exists() {
        planner::ensure_asset_index(&version_json, &index_path, &effective_mirrors, official_first)
            .await
            .map_err(String::from)?;
    }

    let mut requests =
        planner::plan_base(&version_json, &game_root, &effective_mirrors, official_first)
            .map_err(String::from)?;
    let objects = planner::plan_objects(&index_path, &game_root, &effective_mirrors, official_first)
        .await
        .map_err(String::from)?;
    requests.extend(objects);

    // 增量判定：与下载引擎同逻辑（存在 + 哈希匹配即视为完好）
    let mut missing: Vec<crate::download::model::DownloadRequest> = Vec::new();
    for r in requests {
        let ok = match &r.expected {
            Some((algo, hex_str)) => crate::download::hash::verify_file(&r.dest, *algo, hex_str)
                .await
                .unwrap_or(false),
            None => r.dest.exists(),
        };
        if !ok {
            missing.push(r);
        }
    }

    if !missing.is_empty() {
        emit("downloading", &format!("正在补全 {} 个缺失文件…", missing.len()));
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let tx = StdMutex::new(Some(tx));
        let app2 = deps.app.clone();
        let iid2 = instance_id.to_string();
        let cb: ProgressCallback = Arc::new(move |p| {
            let _ = app2.emit("download-progress", &p);
            if matches!(p.state.as_str(), "completed" | "failed" | "cancelled") {
                if let Some(s) = app2.try_state::<AppState>() {
                    s.active_downloads.lock().unwrap().remove(&iid2);
                }
                if let Some(tx) = tx.lock().unwrap().take() {
                    let _ = tx.send(if p.state == "completed" {
                        Ok(())
                    } else {
                        Err(p.error.unwrap_or_else(|| "下载失败".into()))
                    });
                }
            }
        });
        let task_id = format!("res-{}", uuid::Uuid::new_v4().simple());
        deps.download
            .start_task(
                TaskOptions {
                    task_id: task_id.clone(),
                    kind: "resources".into(),
                    instance_id: Some(instance_id.to_string()),
                },
                missing,
                cb,
            )
            .await?;
        rx.await.map_err(|_| "资源补全任务意外结束".to_string())??;
    }

    // ---- 4. natives 解压（固定在版本文件夹下，与隔离开关无关）----
    emit("extracting_natives", "正在解压本地库…");
    let natives_dir = natives::extract_natives(
        &version_json,
        &game_root.join("libraries"),
        &deps.paths.instance_dir(instance_id).join("natives"),
    )
    .await
    .map_err(String::from)?;

    // ---- 5. 分辨率 ----
    let resolution: Option<(u32, u32)> = if detail.jvm.fullscreen {
        app.get_webview_window("main")
            .and_then(|w| w.current_monitor().ok().flatten())
            .map(|m| (m.size().width, m.size().height))
            .or_else(|| Some((1280, 720)))
    } else {
        match (detail.jvm.window_width, detail.jvm.window_height) {
            (Some(w), Some(h)) => Some((w, h)),
            _ => None,
        }
    };

    // ---- 6. 构建命令 ----
    emit("building", "正在组装启动参数…");
    // 直连服务器：SRV 解析（含阻塞 DNS，放阻塞线程）
    let server = if let Some(addr) = &server_address {
        emit("building", &format!("正在解析服务器地址 {addr}…"));
        let target = crate::servers::ping::resolve_target(addr).await.map_err(String::from)?;
        Some((target.host, target.port))
    } else {
        None
    };
    let ctx = args::LaunchContext {
        version_json: &version_json,
        game_root: &game_root,
        game_dir: &game_dir,
        natives_dir: &natives_dir,
        java: &java,
        jvm: &detail.jvm,
        player_name: &player_name,
        auth: &auth,
        extra_jvm: &extra_jvm,
        resolution,
        server,
    };
    let plan = args::build_command(&ctx).map_err(String::from)?;

    // ---- 6.5 确保游戏语言为中文（首次启动预写 options.txt；默认 en_us 时改写） ----
    ensure_chinese_lang(&game_dir).await;

    // ---- 7. 启动进程 ----
    emit("starting", "正在启动游戏进程…");
    let app_log = deps.app.clone();
    let iid_log = instance_id.to_string();
    let on_log: Arc<dyn Fn(&str, String) + Send + Sync> = Arc::new(move |stream: &str, line: String| {
        let _ = app_log.emit(
            "launch-log",
            json!({ "instanceId": iid_log, "stream": stream, "line": line }),
        );
    });

    let app_exit = deps.app.clone();
    let iid_exit = instance_id.to_string();
    let running_map = deps.running.clone();
    let killed_set = deps.killed.clone();
    let log_paths = deps.paths.clone();
    let command_summary = plan.summary.clone();
    // 离线皮肤本地会话服务：游戏退出时关闭（Arc+Mutex 包装以兼容 Fn 闭包）
    let ygg_slot: Arc<StdMutex<Option<crate::account::local_ygg::LocalYggHandle>>> =
        Arc::new(StdMutex::new(ygg_handle));
    let ygg_slot_exit = ygg_slot.clone();
    let on_exit = move |code: Option<i32>, tail: Vec<String>| {
        write_latest_log(&log_paths, &iid_exit, &command_summary, code, &tail);
        if let Some(h) = ygg_slot_exit.lock().unwrap().take() {
            h.shutdown();
        }
        // 强制结束的实例上报 code=null（与自行崩溃区分），前端显示为「已强制结束」
        let was_killed = killed_set.lock().unwrap().remove(&iid_exit);
        let summary = if was_killed {
            Some("已强制结束".to_string())
        } else {
            summarize_exit(code, &tail)
        };
        let _ = app_exit.emit(
            "launch-exited",
            json!({
                "instanceId": iid_exit,
                "code": if was_killed { None::<i32> } else { code },
                "summary": summary,
            }),
        );
        running_map.lock().unwrap().remove(&iid_exit);
    };

    let running = process::spawn_game(&plan, on_log, on_exit).map_err(String::from)?;
    deps.running
        .lock()
        .unwrap()
        .insert(instance_id.to_string(), running.clone());
    // 记录最后启动时间（进程拉起即记；写入失败静默，不影响启动流程）
    deps.instances.touch_launched(instance_id).await;

    let _ = deps.app.emit(
        "launch-state",
        json!({
            "instanceId": instance_id,
            "stage": "running",
            "message": format!("游戏已启动（PID {}）", running.pid),
            "pid": running.pid,
        }),
    );
    Ok(())
}

/// 确保游戏语言为中文：options.txt 缺失时预写 lang:zh_cn（首次启动即中文）；
/// 默认 en_us 时改写为 zh_cn；其他语言视为用户游戏内的主动选择，保留不动
async fn ensure_chinese_lang(game_dir: &std::path::Path) {
    let dir = game_dir.to_path_buf();
    let path = dir.join("options.txt");
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => {
            let has_lang = text.lines().any(|l| l.trim().starts_with("lang:"));
            if has_lang {
                if text.contains("lang:en_us") {
                    let _ = tokio::fs::write(&path, text.replace("lang:en_us", "lang:zh_cn")).await;
                }
            } else {
                let mut out = text;
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("lang:zh_cn\n");
                let _ = tokio::fs::write(&path, out).await;
            }
        }
        Err(_) => {
            if tokio::fs::create_dir_all(&dir).await.is_ok() {
                let _ = tokio::fs::write(&path, "lang:zh_cn\n").await;
            }
        }
    }
}

/// 退出摘要：常见崩溃原因 → 可读文案
fn summarize_exit(code: Option<i32>, tail: &[String]) -> Option<String> {
    if code == Some(0) {
        return None;
    }
    let joined = tail.join("\n");
    if joined.contains("UnsupportedClassVersionError") {
        return Some("Java 版本不匹配：当前 Java 无法运行该版本，请更换 Java".into());
    }
    if joined.contains("OutOfMemoryError") || joined.contains("Could not reserve enough space") {
        return Some("内存不足：请调低实例的最大内存，或关闭其他占用内存的程序".into());
    }
    if joined.contains("MixinApplyError") || joined.contains("FabricLoader") {
        return Some("模组或加载器加载失败，详见日志面板".into());
    }
    Some(format!(
        "游戏异常退出（退出码 {}），详情见日志",
        code.map(|c| c.to_string()).unwrap_or_else(|| "未知".into())
    ))
}

/// 退出时落盘 latest.log：命令摘要 + 退出码 + 最后 200 行日志
fn write_latest_log(
    paths: &LauncherPaths,
    instance_id: &str,
    command_summary: &str,
    code: Option<i32>,
    tail: &[String],
) {
    let log_dir = paths.instance_logs_dir(instance_id);
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let mut content = String::new();
    content.push_str(&format!("// 启动命令：{command_summary}\n"));
    content.push_str(&format!(
        "// 退出码：{}\n\n",
        code.map(|c| c.to_string()).unwrap_or_else(|| "被强制结束".into())
    ));
    content.push_str(&tail.join("\n"));
    let _ = std::fs::write(log_dir.join("latest.log"), content);
}

/// 强制结束实例的游戏进程（收尾与状态恢复由退出监视任务统一完成）
#[tauri::command]
pub async fn kill_instance(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<(), String> {
    let proc = state
        .running
        .lock()
        .unwrap()
        .get(&instance_id)
        .cloned()
        .ok_or_else(|| format!("实例 {instance_id} 未在运行"))?;
    process::terminate(proc.pid)?;
    // 标记为强杀：退出监视任务据此上报 code=null 而非 TerminateProcess 的退出码 1
    state.killed.lock().unwrap().insert(instance_id);
    Ok(())
}

/// 当前运行中的实例状态（前端每 3 秒轮询一次以更新时长与内存）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningState {
    pub instance_id: String,
    pub pid: u32,
    pub started_at: i64,
    pub memory_bytes: Option<u64>,
    pub uptime_secs: i64,
}

#[tauri::command]
pub async fn list_running(state: State<'_, AppState>) -> Result<Vec<RunningState>, String> {
    let snapshot: Vec<(String, Arc<process::RunningProcess>)> = {
        let map = state.running.lock().unwrap();
        map.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    let now = process::now_epoch();
    let mut out = Vec::with_capacity(snapshot.len());
    for (instance_id, p) in snapshot {
        out.push(RunningState {
            instance_id,
            pid: p.pid,
            started_at: p.started_at,
            memory_bytes: process::process_memory(p.pid),
            uptime_secs: now - p.started_at,
        });
    }
    Ok(out)
}
