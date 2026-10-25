// natives classifiers 解压
//
// 仅处理带 natives map 的库（1.18- 的 LWJGL2 风格）：将 natives-windows
// classifier jar 解压到 {版本文件夹}/natives/，供 -Djava.library.path 使用。
// 1.19+ 的 natives-xxx artifact jar 留在 classpath 上由 LWJGL3 自行提取。
// 解压时跳过 META-INF 与签名文件（避免 "Invalid signature file" 崩溃）。

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::core::error::{AppError, AppResult};

use super::rules::{rules_allow, LaunchFeatures};

/// 解压 natives 到 natives_dir（= {版本文件夹}/natives，与隔离开关无关），
/// 返回该目录。无 natives map 的版本（1.19+）也会创建空目录并返回。
pub async fn extract_natives(
    version_json: &Value,
    libraries_root: &Path,
    natives_dir: &Path,
) -> AppResult<PathBuf> {
    tokio::fs::create_dir_all(natives_dir).await?;

    let Some(libs) = version_json.get("libraries").and_then(|x| x.as_array()) else {
        return Ok(natives_dir.to_path_buf());
    };
    let features = LaunchFeatures::default();

    for lib in libs {
        // natives map（仅 Windows 侧）
        let Some(classifier) = lib
            .pointer("/natives/windows")
            .and_then(|x| x.as_str())
            .map(str::to_string)
        else {
            continue;
        };
        if !rules_allow(lib.get("rules"), &features) {
            continue;
        }
        let Some(art) =
            lib.pointer(&format!("/downloads/classifiers/{classifier}"))
        else {
            continue;
        };
        let Some(path) = art.get("path").and_then(|x| x.as_str()) else {
            continue;
        };
        let jar = libraries_root.join(path);
        if !jar.exists() {
            return Err(AppError::msg(format!(
                "natives 库文件缺失：{path}（请先补全资源）"
            )));
        }
        // zip 解压为同步阻塞操作，放阻塞线程池
        let jar = jar.clone();
        let dest = natives_dir.to_path_buf();
        tokio::task::spawn_blocking(move || unzip_native(&jar, &dest))
            .await
            .map_err(|e| AppError::msg(format!("natives 解压任务失败：{e}")))??;
    }
    Ok(natives_dir.to_path_buf())
}

/// 解压单个 natives jar：跳过 META-INF，其余文件平铺写入
fn unzip_native(jar: &Path, dest: &Path) -> AppResult<()> {
    let file = std::fs::File::open(jar)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| AppError::msg(format!("打开 natives jar 失败：{e}")))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| AppError::msg(format!("读取 natives jar 条目失败：{e}")))?;
        let Some(name) = entry.enclosed_name() else {
            continue; // zip-slip 防御
        };
        let name = name.to_string_lossy().into_owned();
        if name.starts_with("META-INF") || name.ends_with(".SF") || name.ends_with(".RSA") {
            continue;
        }
        if entry.is_dir() {
            std::fs::create_dir_all(dest.join(&name))?;
            continue;
        }
        if let Some(p) = dest.join(&name).parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut out = std::fs::File::create(dest.join(&name))?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts: zip::write::SimpleFileOptions = Default::default();
        for (name, data) in entries {
            w.start_file(name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[tokio::test]
    async fn extracts_windows_natives_skips_meta_inf() {
        let dir = std::env::temp_dir().join(format!("piemcl-natives-{}", uuid::Uuid::new_v4()));
        let lib_dir = dir.join("libraries").join("tv/twitch/twitch-platform/5.16");
        std::fs::create_dir_all(&lib_dir).unwrap();

        // 构造 natives jar：含两个 dll + META-INF 签名
        let jar = lib_dir.join("twitch-platform-5.16-natives-windows.jar");
        write_zip(
            &jar,
            &[
                ("lwjgl.dll", b"fake dll A"),
                ("OpenAL64.dll", b"fake dll B"),
                ("META-INF/MANIFEST.MF", b"manifest"),
                ("META-INF/TEST.SF", b"signature"),
            ],
        );

        let v = json!({
            "libraries": [
                {
                    "name": "tv.twitch:twitch-platform:5.16",
                    "natives": { "windows": "natives-windows", "osx": "natives-osx" },
                    "downloads": {
                        "artifact": { "path": "tv/twitch/twitch-platform/5.16/twitch-platform-5.16.jar" },
                        "classifiers": {
                            "natives-windows": { "path": "tv/twitch/twitch-platform/5.16/twitch-platform-5.16-natives-windows.jar" },
                            "natives-osx": { "path": "tv/twitch/twitch-platform/5.16/twitch-platform-5.16-natives-osx.jar" }
                        }
                    }
                },
                {
                    "name": "mac.only:lib:1",
                    "rules": [{ "action": "allow", "os": { "name": "osx" } }],
                    "natives": { "windows": "natives-windows" },
                    "downloads": { "classifiers": { "natives-windows": { "path": "mac/missing.jar" } } }
                }
            ]
        });

        let out = extract_natives(&v, &dir.join("libraries"), &dir.join("natives"))
            .await
            .unwrap();
        assert_eq!(out, dir.join("natives"));
        assert!(out.join("lwjgl.dll").exists());
        assert!(out.join("OpenAL64.dll").exists());
        assert!(!out.join("META-INF").exists(), "签名文件必须跳过");
        // osx classifier 未被使用、mac.only 被 rules 排除（缺失文件不报错）
        assert!(!out.join("missing.jar").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn creates_empty_dir_for_modern_versions() {
        let dir = std::env::temp_dir().join(format!("piemcl-natives2-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // 1.19+：无 natives map
        let v = json!({ "libraries": [
            { "name": "org.lwjgl:lwjgl:3.3.1:natives-windows",
              "downloads": { "artifact": { "path": "lwjgl-natives.jar" } } }
        ]});
        let out = extract_natives(&v, &dir.join("libraries"), &dir.join("natives"))
            .await
            .unwrap();
        assert_eq!(out, dir.join("natives"));
        assert!(out.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
