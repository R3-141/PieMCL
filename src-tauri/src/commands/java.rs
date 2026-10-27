// Java 管理命令
//
// detect_java：扫描系统 Java（PATH / JAVA_HOME / 注册表 / 常见目录 / 启动器目录）
//              并组装各实例的满足状态。实例要求的 Java 优先读冻结 version.json 的
//              javaVersion.majorVersion（Mojang 官方标注，能跟上未来新版本），
//              缺失该字段的极老版本回落 detect::required_java 规则表。
// download_java：Java 页手动下载入口（校验 + 已安装检查后调 install_java）
// install_java：下载 + 解压的共享实现：Adoptium 查询 → 下载引擎拉取（TUNA 镜像优先、
//              SHA-256 校验、断点续传，进度走 download-progress 事件 kind="java"）
//              → 解压落位 → 发 java-installed 事件。阻塞等待全部完成后返回 java.exe
//              路径；启动流程缺 Java 时自动补装复用同一函数。

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use tauri::{AppHandle, Emitter, State};

use super::AppState;
use crate::core::paths::LauncherPaths;
use crate::download::model::{DownloadRequest, HashAlgo};
use crate::download::DownloadEngine;
use crate::java::detect;
use crate::java::install;
use crate::java::model::{DetectJavaResult, InstanceJavaStatus};

/// 实例要求的 Java 主版本：优先读冻结 version.json 的官方标注，缺失回落规则表
async fn instance_required_java(paths: &LauncherPaths, instance_id: &str, mc_version: &str) -> u32 {
    if let Ok(text) =
        tokio::fs::read_to_string(paths.instance_version_json(instance_id)).await
    {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(major) = detect::required_java_from_json(&v) {
                return major;
            }
        }
    }
    detect::required_java(mc_version)
}

/// 手动登记 Java：校验路径可探测后写入 settings.json 的 customJavaPaths，
/// detect_all 时并入扫描列表。路径可为 java.exe 或其所在目录。
#[tauri::command]
pub async fn add_custom_java(
    state: State<'_, AppState>,
    path: String,
) -> Result<crate::java::model::JavaInfo, String> {
    let p = path.trim().to_string();
    if p.is_empty() {
        return Err("路径不能为空".into());
    }
    let probe_path = std::path::PathBuf::from(&p);
    let info = tauri::async_runtime::spawn_blocking(move || detect::probe_java_path(&probe_path))
        .await
        .map_err(|e| format!("探测任务执行失败：{e}"))?
        .ok_or_else(|| "无法识别该路径下的 Java（请选择 java.exe 或其所在目录）".to_string())?;

    let mut settings = state.settings.read().await.clone();
    if settings.custom_java_paths.iter().any(|x| x.eq_ignore_ascii_case(&p)) {
        return Ok(info); // 已登记过，直接返回探测结果
    }
    settings.custom_java_paths.push(p);
    settings
        .validate()
        .map_err(String::from)?;
    settings.save(&state.paths).await?;
    Ok(info)
}

/// 移除手动登记的 Java 路径：仅取消登记（该路径不再纳入检测），不删除文件本身。
/// 路径大小写不敏感匹配；前端移除后自行触发重新检测。
#[tauri::command]
pub async fn remove_custom_java(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let p = path.trim().to_string();
    if p.is_empty() {
        return Err("路径不能为空".into());
    }
    let mut settings = state.settings.read().await.clone();
    let before = settings.custom_java_paths.len();
    settings
        .custom_java_paths
        .retain(|x| !x.eq_ignore_ascii_case(&p));
    if settings.custom_java_paths.len() == before {
        return Err("该路径不在手动登记列表中".into());
    }
    settings.validate().map_err(String::from)?;
    settings.save(&state.paths).await?;
    Ok(())
}

/// 检测系统 Java 与各实例满足状态
#[tauri::command]
pub async fn detect_java(state: State<'_, AppState>) -> Result<DetectJavaResult, String> {
    let paths = state.paths.clone();
    let javas = tauri::async_runtime::spawn_blocking(move || detect::detect_all(&paths))
        .await
        .map_err(|e| format!("检测任务执行失败：{e}"))?;

    let mut instances = Vec::new();
    for summary in state.instances.list().await.map_err(String::from)? {
        let Ok(detail) = state.instances.get(&summary.id).await else {
            continue;
        };
        let required =
            instance_required_java(&state.paths, &summary.id, &summary.mc_version).await;
        let assigned_path = detail.java_path;
        let assigned_version = assigned_path.as_ref().and_then(|p| {
            javas
                .iter()
                .find(|j| j.path.eq_ignore_ascii_case(p))
                .map(|j| j.version)
        });
        let satisfied = match assigned_version {
            Some(v) => v >= required,
            // 未指定 Java 时按"自动选择能否成功"评估
            None => javas.iter().any(|j| j.version >= required),
        };
        instances.push(InstanceJavaStatus {
            instance_id: summary.id,
            instance_name: summary.name,
            mc_version: summary.mc_version,
            required_java: required,
            assigned_path,
            assigned_version,
            satisfied,
        });
    }
    Ok(DetectJavaResult { javas, instances })
}

/// 下载 + 解压指定主版本的 Temurin JRE 到 {root}/java/{major}/（共享实现）。
///
/// 阻塞等待下载与解压全部完成，成功返回 (task_id, 验证通过的 JavaInfo)。
/// 进度经 download-progress 事件推送（kind="java"），解压完成发 java-installed。
/// 探测验证不通过即安装失败（报错携带 java 的实际输出/超时诊断）；
/// 启动流程缺 Java 时自动补装复用同一函数，直接采信返回的 JavaInfo。
/// 同主版本任务互斥（键 java:{major}）。
pub(crate) async fn install_java<R: tauri::Runtime>(
    app: &AppHandle<R>,
    download: &DownloadEngine,
    active_downloads: Arc<StdMutex<HashMap<String, String>>>,
    retry_defs: super::RetryMap,
    paths: &LauncherPaths,
    major_version: u32,
) -> Result<(String, crate::java::model::JavaInfo), String> {
    // 已安装短路：{root}/java/{major}/ 下已有可用运行时则直接返回，
    // 不查 API、不下载、不解压（修复"每次启动重复安装"）。
    // 含重试：运行时已存在时首次执行可能被杀软扫描拖慢；重试 4 次
    // （每次 10s 超时 + 1s 间隔 ≈ 44s 容忍窗口），避免误判后白白重下整个 JRE
    let cache_dir = paths.java_dir().join(major_version.to_string());
    {
        let dir = cache_dir.clone();
        let existing =
            tauri::async_runtime::spawn_blocking(move || {
                crate::java::detect::probe_java_cache_dir_with_retry(&dir, 4)
            })
            .await
            .map_err(|e| format!("探测任务执行失败：{e}"))?;
        if let Some(info) = existing {
            if info.version >= major_version {
                return Ok((format!("java-{major_version}-existing"), info));
            }
            eprintln!(
                "[java] 目录 java/{major_version} 中的运行时主版本不符（{}），重新安装",
                info.version
            );
        }
    }

    // 查询 Adoptium 最新 JRE 资产
    let http = crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent("PieMCL/0.1")
        .connect_timeout(std::time::Duration::from_secs(10)))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败：{e}"))?;
    let (urls, checksum, size, release_name) =
        install::fetch_latest_jre(&http, major_version)
            .await
            .map_err(String::from)?;

    // zip 缓存于 tmp：哈希一致时引擎增量跳过，解压失败可直接重试
    let zip_dest = paths
        .tmp_dir()
        .join(format!("temurin-{major_version}-jre.zip"));
    let mut req = DownloadRequest::new(
        format!("Java {major_version} 运行时（{release_name}）"),
        urls,
        zip_dest.clone(),
    );
    if let Some(c) = checksum {
        req.expected = Some((HashAlgo::Sha256, c));
    }
    req.size = size;

    // 同主版本互斥（复用下载互斥表，键 java:{major}）
    let lock_key = format!("java:{major_version}");
    let task_id = format!("java-{major_version}-{}", uuid::Uuid::new_v4().simple());
    {
        let mut locks = active_downloads.lock().unwrap();
        if let Some(existing) = locks.get(&lock_key) {
            return Err(format!(
                "Java {major_version} 已有任务在进行中（{existing}），请等待完成或先取消"
            ));
        }
        locks.insert(lock_key.clone(), task_id.clone());
    }

    // 登记可重试 + 带重试管理的回调（终态释放互斥 / 保留失败重试定义 / oneshot 通知）
    let opts = crate::download::TaskOptions {
        task_id: task_id.clone(),
        kind: "java".into(),
        instance_id: Some(lock_key.clone()),
    };
    super::register_retry(&retry_defs, &task_id, opts.clone(), vec![req.clone()], Some(lock_key.clone()));
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let tx = std::sync::Mutex::new(Some(tx));
    let cb = super::make_retry_cb(
        retry_defs,
        active_downloads.clone(),
        app.clone(),
        task_id.clone(),
        Some(tx),
    );

    let result = download.start_task(opts, vec![req], cb).await;
    if let Err(e) = result {
        active_downloads.lock().unwrap().remove(&lock_key);
        return Err(e);
    }

    // 等待下载终态
    rx.await
        .map_err(|_| "Java 下载任务意外结束".to_string())?
        .map_err(String::from)?;

    // 解压落位（阻塞操作放 spawn_blocking）
    let dest = paths.java_dir().join(major_version.to_string());
    let zip = zip_dest.clone();
    let extracted =
        tauri::async_runtime::spawn_blocking(move || install::extract_zip(&zip, &dest)).await;
    let java_path = match extracted {
        Ok(Ok(path)) => path,
        Ok(Err(e)) => {
            let _ = app.emit(
                "java-install-failed",
                serde_json::json!({ "major": major_version, "error": e.to_string() }),
            );
            return Err(e.to_string());
        }
        Err(e) => {
            let msg = format!("解压任务执行失败：{e}");
            let _ = app.emit(
                "java-install-failed",
                serde_json::json!({ "major": major_version, "error": msg }),
            );
            return Err(msg);
        }
    };

    // 解压后定向验证探测（顺序、无并行干扰，重试 5 次）：新解压的运行时首次执行
    // 可能被系统文件扫描或磁盘繁忙拖慢，重试直到跑通；最终不通过 = 安装失败，
    // 报错携带诊断信息（超时 / 退出码 / stderr 尾部），不再静默吞掉
    let jp = java_path.clone();
    let verified = tauri::async_runtime::spawn_blocking(move || {
        crate::java::detect::probe_java_path_with_retry_detailed(std::path::Path::new(&jp), 5)
    })
    .await
    .map_err(|e| format!("验证任务执行失败：{e}"))?;
    let info = match verified {
        Ok(info) => {
            eprintln!("[java] 安装验证通过：{}（Java {}）", info.path, info.version);
            info
        }
        Err(d) => {
            let msg = format!("Java {major_version} 已下载解压，但运行时验证失败：{d}");
            let _ = app.emit(
                "java-install-failed",
                serde_json::json!({ "major": major_version, "error": msg }),
            );
            return Err(msg);
        }
    };

    let _ = app.emit(
        "java-installed",
        serde_json::json!({ "major": major_version, "path": info.path }),
    );
    Ok((task_id, info))
}

/// 从 Adoptium 下载指定主版本的 Temurin JRE 并解压到 {root}/java/{major}/
/// 返回 task_id（zip 下载进度见 download-progress 事件）
#[tauri::command]
pub async fn download_java(
    app: AppHandle,
    state: State<'_, AppState>,
    major_version: u32,
) -> Result<String, String> {
    if !(8..=99).contains(&major_version) {
        return Err("Java 主版本需在 8–99 之间".to_string());
    }
    // 已安装则直接提示（幂等入口）
    if let Some(existing) = install::scan_installed(&state.paths.java_dir(), major_version) {
        return Err(format!("Java {major_version} 已安装：{existing}"));
    }

    let (task_id, _) = install_java(
        &app,
        &state.download,
        state.active_downloads.clone(),
        state.retry_defs.clone(),
        &state.paths,
        major_version,
    )
    .await?;
    Ok(task_id)
}
