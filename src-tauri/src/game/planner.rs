// 游戏资源下载规划
//
// 从冻结的 version.json 规划需要下载的文件（增量逻辑在下载引擎内：
// 目标文件已存在且哈希匹配即跳过，只下载缺失或损坏部分）：
// - client.jar     → minecraft/versions/{mc_id}/{mc_id}.jar
// - libraries      → minecraft/libraries/{maven path}（含 rules 过滤与 Windows natives）
// - asset index    → minecraft/assets/indexes/{id}.json
// - assets objects → minecraft/objects/{hash 前 2 位}/{hash}
//                    依赖 index 文件内容，须在 index 就绪后单独规划
//
// URL 候选统一由 download::mirror 生成：镜像优先、官方兜底。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::core::error::{AppError, AppResult};
use crate::download::mirror::build_candidates;
use crate::download::model::{DownloadRequest, HashAlgo};

/// Mojang 版本 rules 过滤（本启动器仅面向 Windows 桌面端）。
/// 语义（与官方启动器 / HMCL 一致）：无 rules 默认允许；
/// 有 rules 时默认排除，按序应用所有匹配规则的 action（后匹配者覆盖），
/// 最终结果决定是否下载 —— 如此"仅 allow osx"的 mac 专属库在 Windows 上被正确排除；
/// 含非空 features 的规则视为不匹配（本启动器未启用任何特性）。
pub fn rules_allow(rules: Option<&Value>) -> bool {
    let Some(arr) = rules.and_then(|r| r.as_array()) else {
        return true;
    };
    let mut allow = false;
    for rule in arr {
        // features 非空 → 本启动器不匹配该规则
        if rule
            .get("features")
            .and_then(|f| f.as_object())
            .is_some_and(|f| !f.is_empty())
        {
            continue;
        }
        // os 规则：仅 name 匹配 "windows"（或无 os / os 为空对象）才视为匹配
        if let Some(os) = rule.get("os") {
            match os.get("name").and_then(|n| n.as_str()) {
                Some("windows") => {}
                Some(_) => continue,
                None => {} // os 缺 name 或为空对象 → 匹配所有平台
            }
        }
        match rule.get("action").and_then(|a| a.as_str()) {
            Some("allow") => allow = true,
            Some("disallow") => allow = false,
            _ => {}
        }
    }
    allow
}

/// 组装单个下载请求
#[allow(clippy::too_many_arguments)]
fn request(
    key: String,
    official_url: &str,
    mirrors: &[String],
    dest: PathBuf,
    sha1: Option<&str>,
    size: Option<u64>,
    official_first: bool,
) -> DownloadRequest {
    let mut r = DownloadRequest::new(
        key,
        build_candidates(mirrors, official_url, official_first),
        dest,
    );
    if let Some(h) = sha1 {
        r.expected = Some((HashAlgo::Sha1, h.to_ascii_lowercase()));
    }
    r.size = size;
    r
}

/// 由 Maven 坐标推导库文件相对路径：
/// group:artifact:version[:classifier] → group/path/artifact/version/artifact-version[-classifier].jar
/// （Fabric / Quilt profile 的 libraries 只有坐标与仓库基址，无 downloads.artifact）
pub(crate) fn maven_path_from_name(name: &str) -> Option<String> {
    // "g:a:v[:classifier][@ext]"：@ext 指定扩展名（缺省 jar），
    // 如 "de.oceanlabs.mcp:mcp_config:1.12.2-...@zip"（Forge 老版映射配置）
    let (name, ext) = match name.split_once('@') {
        Some((n, e)) if !e.is_empty() => (n, e),
        _ => (name, "jar"),
    };
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    let version = parts.next()?;
    let classifier = parts.next();
    let filename = match classifier {
        Some(c) => format!("{artifact}-{version}-{c}.{ext}"),
        None => format!("{artifact}-{version}.{ext}"),
    };
    Some(format!(
        "{}/{}/{}/{}",
        group.replace('.', "/"),
        artifact,
        version,
        filename
    ))
}

/// 追加一个 artifact（library 主文件或 natives classifier）
fn push_artifact(
    out: &mut Vec<DownloadRequest>,
    art: &Value,
    mc_dir: &Path,
    mirrors: &[String],
    official_first: bool,
) {
    let Some(url) = art.get("url").and_then(|v| v.as_str()) else {
        return;
    };
    let Some(path) = art.get("path").and_then(|v| v.as_str()) else {
        return;
    };
    out.push(request(
        format!("libraries/{path}"),
        url,
        mirrors,
        mc_dir.join("libraries").join(path),
        art.get("sha1").and_then(|v| v.as_str()),
        art.get("size").and_then(|v| v.as_u64()),
        official_first,
    ));
}

/// 规划基础资源：client.jar + libraries（含 Windows natives）+ asset index。
/// 不含 assets objects（依赖 index 内容，见 plan_objects）。
pub fn plan_base(
    version: &Value,
    mc_dir: &Path,
    mirrors: &[String],
    official_first: bool,
) -> AppResult<Vec<DownloadRequest>> {
    let mut out = Vec::new();
    let mc_id = version
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // client.jar
    if let Some(client) = version.pointer("/downloads/client") {
        let url = client
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::msg("version.json 缺少 client 下载地址"))?;
        out.push(request(
            format!("{mc_id}.jar"),
            url,
            mirrors,
            mc_dir.join("versions").join(&mc_id).join(format!("{mc_id}.jar")),
            client.get("sha1").and_then(|v| v.as_str()),
            client.get("size").and_then(|v| v.as_u64()),
            official_first,
        ));
    }

    // libraries：主 artifact + Windows natives（供启动阶段解压）
    if let Some(libs) = version.get("libraries").and_then(|v| v.as_array()) {
        for lib in libs {
            if !rules_allow(lib.get("rules")) {
                continue;
            }
            if let Some(art) = lib.pointer("/downloads/artifact") {
                push_artifact(&mut out, art, mc_dir, mirrors, official_first);
            } else if let (Some(name), Some(base)) = (
                lib.get("name").and_then(|v| v.as_str()),
                lib.get("url").and_then(|v| v.as_str()),
            ) {
                // Fabric / Quilt 风格：坐标 + 仓库基址，自行推导路径
                if let Some(rel) = maven_path_from_name(name) {
                    let url = format!("{}/{}", base.trim_end_matches('/'), rel);
                    out.push(request(
                        format!("libraries/{rel}"),
                        &url,
                        mirrors,
                        mc_dir.join("libraries").join(&rel),
                        None,
                        None,
                        official_first,
                    ));
                }
            }
            let natives = lib
                .pointer("/natives/windows")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if let Some(classifier) = natives {
                if let Some(art) =
                    lib.pointer(&format!("/downloads/classifiers/{classifier}"))
                {
                    push_artifact(&mut out, art, mc_dir, mirrors, official_first);
                }
            }
        }
    }

    // asset index
    if let Some(idx) = version.get("assetIndex") {
        if let Some(url) = idx.get("url").and_then(|v| v.as_str()) {
            let index_id = idx.get("id").and_then(|v| v.as_str()).unwrap_or(&mc_id);
            out.push(request(
                format!("assets/indexes/{index_id}.json"),
                url,
                mirrors,
                mc_dir.join("assets").join("indexes").join(format!("{index_id}.json")),
                idx.get("sha1").and_then(|v| v.as_str()),
                idx.get("size").and_then(|v| v.as_u64()),
                official_first,
            ));
        }
    }

    Ok(out)
}

/// 确保 asset index 就绪（objects 规划依赖其内容）：已存在则复用，否则镜像优先下载
pub async fn ensure_asset_index(
    version: &Value,
    dest: &Path,
    mirrors: &[String],
    official_first: bool,
) -> AppResult<()> {
    use std::time::Duration;
    use sha1::{Digest, Sha1};

    let idx = version
        .get("assetIndex")
        .ok_or_else(|| AppError::msg("version.json 缺少 assetIndex 字段"))?;
    let url = idx
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::msg("assetIndex 缺少下载地址"))?;
    let expected_sha1 = idx.get("sha1").and_then(|v| v.as_str());

    let http = crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent("PieMCL/0.1")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60)))
        .build()
        .map_err(|e| AppError::msg(format!("构建 HTTP 客户端失败：{e}")))?;

    let mut last_err: Option<AppError> = None;
    for candidate in build_candidates(mirrors, url, official_first) {
        match http.get(&candidate).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    if let Some(h) = expected_sha1 {
                        let mut d = Sha1::new();
                        Digest::update(&mut d, &bytes);
                        if hex::encode(Digest::finalize(d)) != h.to_ascii_lowercase() {
                            last_err = Some(AppError::msg("资源索引哈希校验失败"));
                            continue;
                        }
                    }
                    if let Some(p) = dest.parent() {
                        tokio::fs::create_dir_all(p).await?;
                    }
                    tokio::fs::write(dest, &bytes).await?;
                    return Ok(());
                }
                Err(e) => last_err = Some(e.into()),
            },
            Ok(resp) => {
                last_err =
                    Some(AppError::msg(format!("资源索引下载失败：服务器返回 {}", resp.status())))
            }
            Err(e) => last_err = Some(e.into()),
        }
    }
    Err(last_err.unwrap_or_else(|| AppError::msg("资源索引下载失败")))
}

/// 从已就绪的 asset index 文件规划 assets objects 下载
pub async fn plan_objects(
    index_path: &Path,
    mc_dir: &Path,
    mirrors: &[String],
    official_first: bool,
) -> AppResult<Vec<DownloadRequest>> {
    let text = tokio::fs::read_to_string(index_path).await?;
    let index: Value = serde_json::from_str(&text)?;
    let objects = index
        .get("objects")
        .and_then(|v| v.as_object())
        .ok_or_else(|| AppError::msg("资源索引内容缺少 objects 字段"))?;

    let mut out = Vec::with_capacity(objects.len());
    for obj in objects.values() {
        let Some(hash) = obj.get("hash").and_then(|v| v.as_str()) else {
            continue;
        };
        let h = hash.to_ascii_lowercase();
        if h.len() != 40 {
            continue; // 非法 sha1，跳过
        }
        let official = format!("https://resources.download.minecraft.net/{}/{}", &h[..2], h);
        out.push(request(
            format!("assets/objects/{h}"),
            &official,
            mirrors,
            mc_dir.join("assets").join("objects").join(&h[..2]).join(&h),
            Some(&h),
            obj.get("size").and_then(|v| v.as_u64()),
            official_first,
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 1.20.1 风格的最小 version.json fixture
    fn fixture_version() -> Value {
        json!({
            "id": "1.20.1",
            "downloads": {
                "client": {
                    "url": "https://piston-data.mojang.com/v1/objects/AAA/client.jar",
                    "sha1": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    "size": 123
                }
            },
            "libraries": [
                {
                    "name": "com.mojang:brigadier:1.0",
                    "downloads": { "artifact": {
                        "path": "com/mojang/brigadier/1.0/brigadier-1.0.jar",
                        "url": "https://libraries.minecraft.net/com/mojang/brigadier/1.0/brigadier-1.0.jar",
                        "sha1": "BBBB", "size": 2
                    }}
                },
                {
                    "name": "org.lwjgl:lwjgl:3.3.1",
                    "rules": [{ "action": "allow", "os": { "name": "windows" } }],
                    "downloads": { "artifact": {
                        "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
                        "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
                        "sha1": "CCCC", "size": 3
                    }}
                },
                {
                    "name": "mac.only:lib:1",
                    "rules": [{ "action": "allow", "os": { "name": "osx" } }],
                    "downloads": { "artifact": {
                        "path": "mac/only/lib.jar",
                        "url": "https://libraries.minecraft.net/mac/only/lib.jar"
                    }}
                },
                {
                    "name": "sample:natives:1",
                    "natives": { "windows": "natives-windows" },
                    "downloads": {
                        "artifact": {
                            "path": "sample/natives/1/natives-1.jar",
                            "url": "https://libraries.minecraft.net/sample/natives/1/natives-1.jar",
                            "sha1": "DDDD", "size": 4
                        },
                        "classifiers": {
                            "natives-windows": {
                                "path": "sample/natives/1/natives-1-windows.jar",
                                "url": "https://libraries.minecraft.net/sample/natives/1/natives-1-windows.jar",
                                "sha1": "EEEE", "size": 5
                            }
                        }
                    }
                },
                {
                    "name": "feature.gated:lib:1",
                    "rules": [{ "action": "allow", "features": { "is_demo_user": true } }],
                    "downloads": { "artifact": {
                        "path": "feature/gated/lib.jar",
                        "url": "https://libraries.minecraft.net/feature/gated/lib.jar"
                    }}
                }
            ],
            "assetIndex": {
                "id": "5",
                "url": "https://piston-meta.mojang.com/v1/packages/xx/5.json",
                "sha1": "FFFF", "size": 10
            }
        })
    }

    const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";

    #[test]
    fn rules_filter_matrix() {
        let v = rules_allow(None);
        assert!(v, "无 rules 默认允许");
        let osx_only = json!([{ "action": "allow", "os": { "name": "osx" } }]);
        assert!(!rules_allow(Some(&osx_only)), "仅允许 osx → Windows 排除");
        let windows_only = json!([{ "action": "allow", "os": { "name": "windows" } }]);
        assert!(rules_allow(Some(&windows_only)), "仅允许 windows → Windows 包含");
        let disallow_windows = json!([{ "action": "disallow", "os": { "name": "windows" } }]);
        assert!(!rules_allow(Some(&disallow_windows)), "禁 windows → 排除");
        let feature_gated =
            json!([{ "action": "allow", "features": { "is_demo_user": true } }]);
        assert!(!rules_allow(Some(&feature_gated)), "feature 规则未启用 → 排除");
        // 成对规则：disallow osx（通用）+ allow osx（mac 变体）在 Windows 上通用条目
        // 实际数据中通用条目不带 rules，此处验证最后匹配覆盖语义
        let paired = json!([
            { "action": "disallow", "os": { "name": "osx" } },
            { "action": "allow", "os": { "name": "osx" } }
        ]);
        assert!(!rules_allow(Some(&paired)), "两条规则均不匹配 windows → 默认排除");
        let windows_then_osx = json!([
            { "action": "allow", "os": { "name": "windows" } },
            { "action": "disallow", "os": { "name": "osx" } }
        ]);
        assert!(rules_allow(Some(&windows_then_osx)), "windows 规则匹配 → 允许");
    }

    #[test]
    fn plan_base_urls_filters_and_paths() {
        let mirrors = vec![BMCLAPI.to_string()];
        let mc_dir = Path::new("C:/mc");
        let reqs = plan_base(&fixture_version(), mc_dir, &mirrors, false).unwrap();

        // client.jar：镜像优先、官方兜底；目标路径与 sha1 小写
        let client = &reqs[0];
        assert_eq!(client.key, "1.20.1.jar");
        assert_eq!(
            client.urls[0],
            format!("{BMCLAPI}/piston-data/v1/objects/AAA/client.jar")
        );
        assert_eq!(
            client.urls.last().unwrap(),
            "https://piston-data.mojang.com/v1/objects/AAA/client.jar"
        );
        assert_eq!(client.dest, mc_dir.join("versions/1.20.1/1.20.1.jar"));
        assert_eq!(
            client.expected,
            Some((HashAlgo::Sha1, "a".repeat(40)))
        );
        assert_eq!(client.size, Some(123));

        // mac.only 被排除；feature.gated 被排除；lwjgl 与 brigadier 保留
        let keys: Vec<&str> = reqs.iter().map(|r| r.key.as_str()).collect();
        assert!(!keys.iter().any(|k| k.contains("mac/only")));
        assert!(!keys.iter().any(|k| k.contains("feature/gated")));
        assert!(keys.iter().any(|k| k.contains("brigadier")));
        assert!(keys.iter().any(|k| k.contains("lwjgl")));

        // natives classifier 一并规划
        let natives = reqs
            .iter()
            .find(|r| r.key.contains("natives-1-windows"))
            .expect("应包含 Windows natives");
        assert_eq!(natives.dest, mc_dir.join("libraries/sample/natives/1/natives-1-windows.jar"));

        // asset index 路径
        let index = reqs
            .iter()
            .find(|r| r.key == "assets/indexes/5.json")
            .expect("应包含 asset index");
        assert_eq!(index.dest, mc_dir.join("assets/indexes/5.json"));
        assert_eq!(index.urls[0], format!("{BMCLAPI}/piston-meta/v1/packages/xx/5.json"));
    }

    #[tokio::test]
    async fn plan_objects_from_index_file() {
        let dir = std::env::temp_dir().join(format!("piemcl-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let index_path = dir.join("5.json");
        std::fs::write(
            &index_path,
            json!({
                "objects": {
                    "minecraft/sounds/click.ogg": { "hash": "ABCDABCDABCDABCDABCDABCDABCDABCDABCDABCD", "size": 100 },
                    "pack.png": { "hash": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef", "size": 200 }
                }
            })
            .to_string(),
        )
        .unwrap();

        let mirrors = vec![BMCLAPI.to_string()];
        let reqs = plan_objects(&index_path, &dir, &mirrors, false).await.unwrap();
        assert_eq!(reqs.len(), 2);

        let first = &reqs[0];
        assert_eq!(first.dest, dir.join("assets/objects/ab/abcdabcdabcdabcdabcdabcdabcdabcdabcdabcd"));
        assert_eq!(
            first.urls.last().unwrap(),
            "https://resources.download.minecraft.net/ab/abcdabcdabcdabcdabcdabcdabcdabcdabcdabcd"
        );
        assert_eq!(first.urls[0], format!("{BMCLAPI}/assets/ab/abcdabcdabcdabcdabcdabcdabcdabcdabcdabcd"));
        assert_eq!(first.size, Some(100));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // 网络冒烟测试（默认忽略，手动运行：cargo test -- --ignored）
    // 真实网络：创建原版实例 → 补全 client.jar + libraries + asset index
    //          → 抽样下载 5 个 assets objects（全量 400MB+ 太大）
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn vanilla_install_smoke() {
        use crate::core::paths::LauncherPaths;
        use crate::core::settings::DEFAULT_MIRROR;
        use crate::download::hash;
        use crate::download::DownloadEngine;
        use crate::instance::model::LoaderInfo;
        use crate::instance::store::InstanceStore;
        use crate::version::resolve::fetch_version_resolved;
        use crate::version::repo::VersionRepo;
        use std::sync::mpsc::channel;

        let root =
            std::env::temp_dir().join(format!("piemcl-smoke-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = LauncherPaths::new(root.clone(), root.join(".minecraft"));
        paths.ensure_dirs().await.unwrap();
        let repo = VersionRepo::new(paths.clone());
        let store = InstanceStore::new(paths.clone());

        // 1. 获取并冻结版本元数据，创建实例
        let mc = "1.21.1";
        let resolved = fetch_version_resolved(&repo, mc).await.expect("获取版本元数据失败");
        let summary = store
            .create("smoke-vanilla", mc, &resolved, LoaderInfo::vanilla())
            .await
            .expect("创建实例失败");
        let game_root = paths.game_root().to_path_buf();

        let engine = DownloadEngine::new(paths.tmp_dir(), 32, 16);
        let mirrors = vec![DEFAULT_MIRROR.to_string()];

        // 2. 第一轮：client.jar + libraries + asset index
        let requests = plan_base(&resolved, &game_root, &mirrors, false).unwrap();
        let (tx, rx) = channel();
        let cb: crate::download::ProgressCallback =
            std::sync::Arc::new(move |p| {
                let _ = tx.send(p);
            });
        engine
            .start_task(
                crate::download::TaskOptions {
                    task_id: "smoke-1".into(),
                    kind: "test".into(),
                    instance_id: None,
                },
                requests,
                cb,
            )
            .await
            .unwrap();

        // 等待完成（真实网络，放宽到 10 分钟）
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        loop {
            let remain = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remain.is_zero(), "第一轮下载超时");
            let ev = rx.recv_timeout(remain).expect("接收事件失败");
            if ev.state == "completed" {
                break;
            }
            assert_ne!(ev.state, "failed", "第一轮失败：{:?}", ev.error);
        }

        // client.jar 存在且 sha1 匹配（落在版本文件夹：versions/{实例名}/{实例名}.jar）
        let client_dest = game_root
            .join("versions")
            .join(&summary.id)
            .join(format!("{}.jar", summary.id));
        let expected_sha1 = resolved["downloads"]["client"]["sha1"].as_str().unwrap();
        assert!(
            hash::verify_file(&client_dest, HashAlgo::Sha1, expected_sha1).await.unwrap(),
            "client.jar 哈希不匹配"
        );

        // 3. 第二轮：抽样 5 个 assets objects
        let index_id = resolved["assetIndex"]["id"].as_str().unwrap();
        let index_path = game_root.join("assets").join("indexes").join(format!("{index_id}.json"));
        assert!(index_path.exists(), "asset index 应已就绪");
        let mut objects = plan_objects(&index_path, &game_root, &mirrors, false).await.unwrap();
        assert!(objects.len() > 100, "1.21.1 的 objects 应有上千个");
        objects.truncate(5);
        let objects_dests: Vec<PathBuf> = objects.iter().map(|r| r.dest.clone()).collect();

        let (tx2, rx2) = channel();
        let cb2: crate::download::ProgressCallback =
            std::sync::Arc::new(move |p| {
                let _ = tx2.send(p);
            });
        engine
            .start_task(
                crate::download::TaskOptions {
                    task_id: "smoke-2".into(),
                    kind: "test".into(),
                    instance_id: None,
                },
                objects,
                cb2,
            )
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let remain = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remain.is_zero(), "第二轮下载超时");
            let ev = rx2.recv_timeout(remain).expect("接收事件失败");
            if ev.state == "completed" {
                break;
            }
            assert_ne!(ev.state, "failed", "第二轮失败：{:?}", ev.error);
        }
        // objects 哈希校验已由引擎完成；文件均已落位到目标路径
        assert!(objects_dests.iter().all(|d| d.exists()), "objects 应全部落位");

        let _ = std::fs::remove_dir_all(&root);
    }

    // -----------------------------------------------------------------------
    // Fabric 安装冒烟测试（默认忽略，手动运行：cargo test -- --ignored fabric）
    // 真实网络：拉取 Fabric profile → 与 MC version.json 合并 → 下载 loader libraries
    // -----------------------------------------------------------------------
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn fabric_install_smoke() {
        use crate::core::paths::LauncherPaths;
        use crate::core::settings::DEFAULT_MIRROR;
        use crate::download::{DownloadEngine, TaskOptions};
        use crate::version::resolve::fetch_version_resolved;
        use crate::version::repo::VersionRepo;
        use std::sync::mpsc::channel;

        let root =
            std::env::temp_dir().join(format!("piemcl-fabric-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = LauncherPaths::new(root.clone(), root.join(".minecraft"));
        paths.ensure_dirs().await.unwrap();
        let repo = VersionRepo::new(paths.clone());

        let mc = "1.21.1";
        let resolved = fetch_version_resolved(&repo, mc).await.expect("获取 MC 元数据失败");

        // 1. 取最新 Fabric loader 版本并拉取 profile
        let versions = repo.list_loader_versions("fabric", mc).await.expect("获取 loader 版本失败");
        let loader_ver = versions.first().expect("Fabric 版本列表为空").clone();
        let profile = repo
            .fetch_loader_profile("fabric", mc, &loader_ver)
            .await
            .expect("拉取 Fabric profile 失败");
        assert!(profile.get("inheritsFrom").is_some(), "profile 应继承 MC 版本");

        // 2. 合并（install_loader 内同款逻辑）：MC 冻结版 + profile
        let mut merged = crate::version::resolve::merge_version_json(&resolved, &profile);
        if let Some(obj) = merged.as_object_mut() {
            obj.remove("inheritsFrom");
            obj.insert("id".to_string(), serde_json::json!(mc));
        }
        // mainClass 应为 Fabric KnotClient，libraries 数量只增不减，id 保持 MC 版本
        assert_eq!(
            merged["mainClass"].as_str().unwrap(),
            "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "mainClass 应为 Fabric KnotClient"
        );
        assert_eq!(merged["id"].as_str().unwrap(), mc);
        let mc_libs = resolved["libraries"].as_array().unwrap().len();
        let merged_libs = merged["libraries"].as_array().unwrap().len();
        assert!(merged_libs > mc_libs, "合并后 libraries 应增加（{} → {}）", mc_libs, merged_libs);

        // 3. 规划 loader libraries（profile 无 downloads 字段 → 不含 client.jar）并下载
        let requests =
            plan_base(&profile, &root.join("mc"), &[DEFAULT_MIRROR.to_string()], false).unwrap();
        assert!(
            requests.iter().all(|r| !r.key.ends_with(".jar") || r.key.contains("libraries/")),
            "profile 规划不应包含 client.jar"
        );
        assert!(!requests.is_empty(), "Fabric loader libraries 不应为空");
        assert!(
            requests.iter().all(|r| r.urls[0].starts_with("https://maven.fabricmc.net/")),
            "Fabric 库应走官方 maven（非 Mojang 域名不改写镜像）"
        );

        let engine = DownloadEngine::new(paths.tmp_dir(), 16, 8);
        let (tx, rx) = channel();
        let cb: crate::download::ProgressCallback = std::sync::Arc::new(move |p| {
            let _ = tx.send(p);
        });
        engine
            .start_task(
                TaskOptions { task_id: "smoke-fabric".into(), kind: "test".into(), instance_id: None },
                requests.clone(),
                cb,
            )
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
        loop {
            let remain = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remain.is_zero(), "Fabric libraries 下载超时");
            let ev = rx.recv_timeout(remain).expect("接收事件失败");
            if ev.state == "completed" {
                break;
            }
            assert_ne!(ev.state, "failed", "Fabric libraries 下载失败：{:?}", ev.error);
        }
        assert!(
            requests.iter().all(|r| r.dest.exists()),
            "Fabric libraries 应全部落位"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
