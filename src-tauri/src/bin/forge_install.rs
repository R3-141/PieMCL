// Forge / NeoForge 真实安装矩阵冒烟工具（开发用途，不随应用打包）
//
// 用法（在 src-tauri 下）：
//   cargo run --bin forge_install -- <forge|neoforge> <mc_version> <full_version> <tag>
// 例：
//   cargo run --bin forge_install -- forge 1.7.10 10.13.4.1614 1710
//   cargo run --bin forge_install -- neoforge 1.21.1 21.1.264 nf1211
//
// 复用 dev 数据根（AppData/com.piemcl.launcher）：已装 Java、已下载 assets/
// libraries 增量跳过，installer 有全局缓存。实例写入
// {game_root}/versions/mctest-{tag}，成功后删除（失败保留现场便于排查）。
// app 传 None（静默，不发事件；Java 已装时不走补装分支）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use piemcl_lib::core::paths::LauncherPaths;
use piemcl_lib::download::DownloadEngine;
use piemcl_lib::loader::installer::{ForgeInstallCtx, install_forge_like};

const TEST_MIRROR: &str = piemcl_lib::core::settings::DEFAULT_MIRROR;

fn dev_paths() -> LauncherPaths {
    let base = PathBuf::from(std::env::var("APPDATA").expect("需要 APPDATA 环境变量"))
        .join("com.piemcl.launcher");
    LauncherPaths::new(base.clone(), base.join(".minecraft"))
}

/// 拉取原版 version.json：优先缓存（cache/versions/{mc}/{mc}.json），
/// 缺失则走 BMCLAPI 版本清单镜像下载并落缓存
async fn fetch_vanilla_json(paths: &LauncherPaths, mc: &str) -> serde_json::Value {
    let dir = paths.cache_dir().join("versions").join(mc);
    let path = dir.join(format!("{mc}.json"));
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str(&text) {
            return v;
        }
    }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("HTTP 客户端");
    let manifest: serde_json::Value = http
        .get(format!("{TEST_MIRROR}/mc/game/version_manifest_v2.json"))
        .send()
        .await
        .expect("版本清单下载失败")
        .json()
        .await
        .expect("版本清单解析失败");
    let url = manifest["versions"]
        .as_array()
        .and_then(|vs| {
            vs.iter()
                .find(|v| v["id"].as_str() == Some(mc))
                .and_then(|v| v["url"].as_str())
        })
        .unwrap_or_else(|| panic!("版本清单中未找到 {mc}"))
        .to_string();
    // piston-meta / launchermeta 统一换 BMCLAPI 镜像（同路径）
    let mirrored = url
        .replacen("https://piston-meta.mojang.com", TEST_MIRROR, 1)
        .replacen("https://launchermeta.mojang.com", TEST_MIRROR, 1);
    let text = http
        .get(&mirrored)
        .send()
        .await
        .expect("原版 json 下载失败")
        .text()
        .await
        .expect("原版 json 读取失败");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, &text).unwrap();
    serde_json::from_str(&text).expect("原版 json 解析失败")
}

/// 现代管线：jvm 数组中重复 flag 必须保留（--add-opens 的 value 不能失去归属）
fn assert_no_orphan_flag_values(vjson: &serde_json::Value, tag: &str) {
    let Some(arr) = vjson.pointer("/arguments/jvm").and_then(|v| v.as_array()) else {
        return; // 老版本走 minecraftArguments，无 jvm 数组
    };
    let toks: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
    // 任何裸 value（形如 模块/包=目标）不得连续出现两次（flag 被去重丢失的形态）
    let mut consecutive_values = 0;
    for t in &toks {
        if t.starts_with('-') {
            consecutive_values = 0;
        } else if t.contains('=') && !t.contains("${") {
            consecutive_values += 1;
            assert!(
                consecutive_values <= 1,
                "[{tag}] 出现连续裸 value（flag 疑被去重丢失）：{toks:?}"
            );
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (loader_type, mc, full, tag) = match args.as_slice() {
        [_, lt, mc, full, tag] => (lt.clone(), mc.clone(), full.clone(), tag.clone()),
        _ => {
            eprintln!("用法：forge_install <forge|neoforge> <mc_version> <full_version> <tag>");
            std::process::exit(2);
        }
    };

    let paths = dev_paths();
    let instance_id = format!("mctest-{tag}");
    let inst_dir = paths.instance_dir(&instance_id);
    let _ = std::fs::remove_dir_all(&inst_dir); // 清理上次残留

    let version_json = fetch_vanilla_json(&paths, &mc).await;
    let (active, retry_defs) = ForgeInstallCtx::new_maps();
    let ctx = ForgeInstallCtx {
        app: None, // 静默：无事件推送，Java 已装时不走补装分支
        download: DownloadEngine::new(paths.tmp_dir(), 16, 8),
        active,
        retry_defs,
        task_id: format!("t-{}", uuid::Uuid::new_v4().simple()),
        instance_id: instance_id.clone(),
        paths: paths.clone(),
        game_root: paths.game_root().to_path_buf(),
        version_json,
        loader_type: loader_type.clone(),
        full_version: full.clone(),
        mc_version: mc.clone(),
        mirrors: vec![TEST_MIRROR.to_string()],
        official_first: true,
    };

    println!("== 开始安装 {loader_type} {full}（实例 {instance_id}）==");
    if let Err(e) = install_forge_like(ctx).await {
        eprintln!("[{tag}] {loader_type} {full} 安装失败：{e}");
        eprintln!("实例目录已保留现场：{}", inst_dir.display());
        std::process::exit(1);
    }

    // 产物断言：主 jar 存在且非空
    let main_jar = inst_dir.join(format!("{instance_id}.jar"));
    let jar_size = std::fs::metadata(&main_jar)
        .unwrap_or_else(|e| panic!("[{tag}] 主 jar 不存在：{e}"))
        .len();
    assert!(jar_size > 2 * 1024 * 1024, "[{tag}] 主 jar 过小：{jar_size}");

    // version.json：id 必须等于实例名 + 启动参数无孤儿 flag
    let vjson: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(inst_dir.join(format!("{instance_id}.json")))
            .expect("version.json 缺失"),
    )
    .expect("version.json 解析失败");
    assert_eq!(vjson["id"], instance_id, "version.json id 应等于实例名");
    assert_no_orphan_flag_values(&vjson, &tag);

    println!("== {loader_type} {full} 安装验证通过 ==");
    println!("   mainClass = {}", vjson["mainClass"]);
    if let Some(args) = vjson["minecraftArguments"].as_str() {
        println!("   minecraftArguments 含 tweaker = {}", args.contains("Tweaker"));
    }
    if let Some(arr) = vjson.pointer("/arguments/jvm").and_then(|v| v.as_array()) {
        let opens = arr
            .iter()
            .filter(|v| v.as_str() == Some("--add-opens"))
            .count();
        println!("   jvm tokens = {}（--add-opens x{opens}）", arr.len());
    }

    // 清理测试实例（保留 cache/forge 与 libraries 供增量复用）
    let _ = std::fs::remove_dir_all(&inst_dir);
    println!("== 完成，测试实例已清理 ==");
}
