// Adoptium Temurin 自动下载安装
//
// 流程：Adoptium API 查询最新 x64 Windows JRE → 下载引擎拉取 zip
//       （SHA-256 校验、断点续传，进度经 download-progress 事件推送，kind = "java"）
//       → 完成回调 spawn_blocking 解压到 {root}/java/{major}/
//       → 成功发 java-installed 事件，失败发 java-install-failed 事件。
// 下载源：API 只给 GitHub 链接（国内慢），故组装候选列表
//       [清华 TUNA 镜像（同文件同校验值），GitHub 官方兜底]，
//       引擎重试时自动轮换，镜像失败自动回退官方。

use std::path::Path;

use serde_json::Value;

use crate::core::error::{AppError, AppResult};

/// Adoptium API：最新发行版资产查询
const ADOPTIUM_API: &str = "https://api.adoptium.net/v3/assets/latest";
/// 清华 TUNA 的 Adoptium 镜像（文件与官方一致，校验值相同）
const TUNA_ADOPTIUM: &str = "https://mirrors.tuna.tsinghua.edu.cn/Adoptium";

/// 查询 Adoptium 最新 JRE 资产（x64 / windows / hotspot）
/// 返回 (下载候选列表[镜像优先、官方兜底], sha256 校验, 字节数, release 名称)
pub async fn fetch_latest_jre(
    http: &reqwest::Client,
    major: u32,
) -> AppResult<(Vec<String>, Option<String>, Option<u64>, String)> {
    let url = format!(
        "{ADOPTIUM_API}/{major}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse"
    );
    let resp = http
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AppError::msg(format!("Adoptium API 请求失败：{e}")))?;
    let arr: Vec<Value> = resp.json().await.map_err(|_| {
        AppError::msg(format!("Adoptium 未提供 Java {major} 的 Windows x64 JRE"))
    })?;
    let asset = arr.first().ok_or_else(|| {
        AppError::msg(format!("Adoptium 未提供 Java {major} 的 Windows x64 JRE"))
    })?;
    let package_name = asset
        .pointer("/binary/package/name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::msg("Adoptium 响应缺少包名"))?
        .to_string();
    let link = asset
        .pointer("/binary/package/link")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::msg("Adoptium 响应缺少下载链接"))?
        .to_string();
    // checksum 形如 "sha256:xxxx"，去掉前缀统一小写
    let checksum = asset
        .pointer("/binary/package/checksum")
        .and_then(|v| v.as_str())
        .map(|c| {
            c.trim()
                .trim_start_matches("sha256:")
                .trim_start_matches("SHA256:")
                .to_ascii_lowercase()
        });
    let size = asset.pointer("/binary/package/size").and_then(|v| v.as_u64());
    let release_name = asset
        .get("release_name")
        .and_then(|v| v.as_str())
        .unwrap_or("temurin")
        .to_string();
    // 候选：TUNA 镜像优先（同文件、同 sha256），GitHub 官方兜底
    let tuna = format!("{TUNA_ADOPTIUM}/{major}/jre/x64/windows/{package_name}");
    Ok((vec![tuna, link], checksum, size, release_name))
}

/// 在目录内查找 bin/java.exe（支持 {dir}/bin 与 {dir}/*/bin 两种布局）
pub fn find_java_exe(dir: &Path) -> Option<String> {
    let direct = dir.join("bin").join("java.exe");
    if direct.is_file() {
        return Some(direct.to_string_lossy().to_string());
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let candidate = e.path().join("bin").join("java.exe");
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// 查询指定主版本是否已通过启动器安装；返回 java.exe 路径
pub fn scan_installed(java_root: &Path, major: u32) -> Option<String> {
    find_java_exe(&java_root.join(major.to_string()))
}

/// 解压 zip 到目标目录（enclosed_name 已防 zip-slip）；
/// 返回解压后找到的 bin/java.exe 路径。目录残留（上次中断）会先清理。
pub fn extract_zip(zip_path: &Path, dest_dir: &Path) -> AppResult<String> {
    if dest_dir.exists() {
        std::fs::remove_dir_all(dest_dir)?;
    }
    std::fs::create_dir_all(dest_dir)?;

    let file = std::fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| AppError::msg(format!("打开 zip 失败：{e}")))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| AppError::msg(format!("读取 zip 条目失败：{e}")))?;
        let Some(name) = entry.enclosed_name() else {
            continue; // 含 ../ 或绝对路径的可疑条目，跳过
        };
        let out = dest_dir.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut f = std::fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut f)?;
        }
    }
    find_java_exe(dest_dir)
        .ok_or_else(|| AppError::msg("解压完成但未找到 bin/java.exe（压缩包可能不完整）"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 在内存中构造一个 zip 文件（含子目录布局 + 一条 zip-slip 恶意条目）
    fn build_test_zip() -> Vec<u8> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(buf);
        let opts: zip::write::SimpleFileOptions = Default::default();
        w.add_directory("jdk-21/", opts).unwrap();
        w.start_file("jdk-21/release", opts).unwrap();
        w.write_all(b"JAVA_VERSION=\"21.0.5\"").unwrap();
        w.start_file("jdk-21/bin/java.exe", opts).unwrap();
        w.write_all(b"fake java binary").unwrap();
        w.start_file("../evil.txt", opts).unwrap();
        w.write_all(b"zip slip attempt").unwrap();
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn extract_finds_java_and_blocks_zip_slip() {
        let dir = std::env::temp_dir().join(format!("piemcl-zip-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("test.zip");
        std::fs::write(&zip_path, build_test_zip()).unwrap();

        let dest = dir.join("jre");
        let java = extract_zip(&zip_path, &dest).unwrap();
        assert!(java.ends_with("java.exe"), "应返回 java.exe 路径：{java}");
        assert!(dest.join("jdk-21/release").exists());
        assert!(
            !dir.join("evil.txt").exists(),
            "zip-slip 条目必须被拒绝"
        );

        // 重复解压：残留目录先清理，不报错
        let java2 = extract_zip(&zip_path, &dest).unwrap();
        assert_eq!(java, java2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_installed_detects_layouts() {
        let dir = std::env::temp_dir().join(format!("piemcl-scan-{}", uuid::Uuid::new_v4()));
        // 布局一：{major}/bin/java.exe
        let a = dir.join("21").join("bin");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::write(a.join("java.exe"), b"x").unwrap();
        assert!(scan_installed(&dir, 21).is_some());
        assert!(scan_installed(&dir, 17).is_none());
        // 布局二：{major}/{子目录}/bin/java.exe
        let b = dir.join("17").join("jdk-17.0.2+8").join("bin");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("java.exe"), b"x").unwrap();
        assert!(scan_installed(&dir, 17).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -----------------------------------------------------------------------
    // 真实系统实测（默认忽略，手动运行：cargo test -- --ignored）
    // -----------------------------------------------------------------------

    /// 真实扫描系统 Java（输出供人工核对；CI 无 Java 也不失败）
    #[test]
    #[ignore]
    fn detect_real_system_java() {
        let paths = crate::core::paths::LauncherPaths::new(
            std::env::temp_dir().join("piemcl-java-detect"),
            std::env::temp_dir().join("piemcl-java-detect").join(".minecraft"),
        );
        std::fs::create_dir_all(paths.java_dir()).unwrap();
        let javas = crate::java::detect::detect_all(&paths);
        println!("检测到 {} 个 Java：", javas.len());
        for j in &javas {
            println!("  Java {} [{}] {} @ {}", j.version, j.arch, j.vendor, j.path);
        }
    }

    /// 真实下载 Adoptium Java 21 JRE → SHA-256 校验 → 解压 → 执行 -version 验证
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn download_java21_smoke() {
        use crate::core::paths::LauncherPaths;
        use crate::download::model::{DownloadRequest, HashAlgo};
        use crate::download::{DownloadEngine, TaskOptions};
        use crate::java::detect;
        use std::sync::mpsc::channel;

        let root =
            std::env::temp_dir().join(format!("piemcl-java21-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = LauncherPaths::new(root.clone(), root.join(".minecraft"));
        paths.ensure_dirs().await.unwrap();

        // 1. 查询 Adoptium 最新 Java 21 JRE（TUNA 镜像优先、GitHub 兜底）
        let http = crate::core::net::apply_proxy(reqwest::Client::builder()
            .user_agent("PieMCL/0.1")
            .connect_timeout(std::time::Duration::from_secs(10)))
            .build()
            .unwrap();
        let (urls, checksum, size, name) =
            fetch_latest_jre(&http, 21).await.expect("查询 Adoptium 失败");
        println!("目标资产：{name}（约 {} MB），候选源：{urls:?}", size.unwrap_or(0) / 1024 / 1024);

        // 2. 经下载引擎拉取（SHA-256 校验 + 分块 + 断点续传）
        let zip_dest = paths.tmp_dir().join("temurin-21-jre.zip");
        let mut req = DownloadRequest::new("Java 21 运行时", urls, zip_dest.clone());
        if let Some(c) = checksum {
            req.expected = Some((HashAlgo::Sha256, c));
        }
        req.size = size;

        let engine = DownloadEngine::new(paths.tmp_dir(), 8, 4);
        let (tx, rx) = channel();
        let cb: crate::download::ProgressCallback = std::sync::Arc::new(move |p| {
            let _ = tx.send(p);
        });
        engine
            .start_task(
                TaskOptions { task_id: "smoke-java21".into(), kind: "test".into(), instance_id: None },
                vec![req],
                cb,
            )
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
        loop {
            let remain = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remain.is_zero(), "Java 21 下载超时");
            let ev = rx.recv_timeout(remain).expect("接收事件失败");
            if ev.state == "completed" {
                break;
            }
            assert_ne!(ev.state, "failed", "Java 21 下载失败：{:?}", ev.error);
        }

        // 3. 解压并验证可执行
        let java_root = root.join("java");
        let java_exe =
            extract_zip(&zip_dest, &java_root.join("21")).expect("解压失败");
        assert!(std::path::Path::new(&java_exe).is_file(), "java.exe 应存在：{java_exe}");

        let output = std::process::Command::new(&java_exe)
            .arg("-XshowSettings:properties")
            .arg("-version")
            .output()
            .expect("执行 java.exe 失败");
        let text = String::from_utf8_lossy(&output.stderr);
        let (v, arch, vendor) = detect::parse_probe_output(&text).expect("解析输出失败");
        assert_eq!(v, 21, "应为 Java 21");
        assert_eq!(arch, "x64", "Adoptium x64 包应为 x64 架构");
        println!("安装成功：{java_exe}（Java {v} / {arch} / {vendor}）");

        // 4. scan_installed 能发现（供 detect_java 收录）
        assert_eq!(
            scan_installed(&java_root, 21).as_deref(),
            Some(java_exe.as_str())
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
