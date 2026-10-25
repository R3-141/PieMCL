// Java 运行时检测
//
// 扫描来源（去重后逐一探测）：
// - PATH 环境变量各目录
// - JAVA_HOME
// - 注册表 JavaSoft（JDK / JRE 各版本子键的 JavaHome）
// - Program Files 常见发行版目录
// - 启动器自动下载目录 {root}/java/{major}
// 探测：执行 `java -XshowSettings:properties -version`（输出在 stderr），
//       解析 java.version / os.arch / java.vendor。
// 阻塞操作（进程执行、注册表、目录遍历）——调用方须放 spawn_blocking。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use super::model::JavaInfo;
use crate::core::paths::LauncherPaths;

/// 单次探测超时
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// MC 版本要求的最低 Java 主版本（规则表兜底）
///
/// 规则表（与主流启动器一致）：
/// - 26.x（新版本号方案）    → 25
/// - 1.20.5+ / 1.21+        → 21
/// - 1.18 – 1.20.4          → 17
/// - 1.17.x                 → 17（官方要求 16，Java 17 为其超集，且与 1.18+ 统一减少下载版本）
/// - 1.16.x 及更早 / 经典版 → 8
///
/// 注意：新版本一律优先用 required_java_from_json 读官方标注，此表仅兜底
/// 极老版本（version.json 无 javaVersion 字段），保证规则表永远不必追新。
pub fn required_java(mc_version: &str) -> u32 {
    let parts: Vec<&str> = mc_version.trim().split('.').collect();
    if parts.len() >= 2 {
        if let Ok(first) = parts[0].parse::<u32>() {
            if first == 1 {
                let minor: u32 = parts[1].parse().unwrap_or(0);
                let patch: u32 = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
                return if minor > 20 || (minor == 20 && patch >= 5) {
                    21
                } else if minor >= 17 {
                    17
                } else {
                    8
                };
            }
            // 新版本号方案（26.x 起年份式版本）：走高版本 Java
            return if first >= 26 { 25 } else { 21 };
        }
    }
    // 经典版（rd-xxx / a1.x / b1.x 等）
    8
}

/// 从冻结 version.json 读取 Mojang 官方标注的 Java 要求（javaVersion.majorVersion）。
/// 这是唯一能跟上未来新版本的可信来源；返回 None 表示该版本无此字段（需规则表兜底）。
pub fn required_java_from_json(version_json: &serde_json::Value) -> Option<u32> {
    version_json
        .pointer("/javaVersion/majorVersion")
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
}

/// 旧版 Minecraft 的 Java 版本上限（老加载器工具链与新 Java 不兼容）：
/// - 1.16.x 及更早 → Some(8)：launchwrapper 把系统 ClassLoader 强转
///   URLClassLoader，Java 9+ 直接 ClassCastException 崩溃；1.13–1.16 官方
///   推荐同为 Java 8
/// - 1.17+        → None（满足下限即可，无上限）
/// - 26.x（新版本号方案，年份式版本号）→ None：高版本 Java，无上限
/// - 经典版（rd-xxx / a1.x / b1.x 等非纯数字首段）→ Some(8)
pub fn max_java(mc_version: &str) -> Option<u32> {
    let parts: Vec<&str> = mc_version.trim().split('.').collect();
    if parts.len() >= 2 {
        if let Ok(first) = parts[0].parse::<u32>() {
            if first == 1 {
                let minor: u32 = parts[1].parse().unwrap_or(0);
                // 1.16.x 及更早 → 上限 8；1.17+ → 无上限
                return if minor < 17 { Some(8) } else { None };
            }
            // 新版本号方案（26.x 起年份式版本）：要求高 Java，无上限
            return None;
        }
    }
    // 经典版兜底
    Some(8)
}

/// 解析 java.version 字符串 → 主版本号
/// "21.0.5" → 21；"17.0.2" → 17；"1.8.0_401" → 8（旧版命名）
pub fn parse_java_version(v: &str) -> Option<u32> {
    let mut parts = v.trim().split('.');
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// os.arch 归一化：amd64/x86_64 → x64；x86 系 → x86；arm64 系 → aarch64
pub fn normalize_arch(a: &str) -> Option<String> {
    match a.to_ascii_lowercase().as_str() {
        "amd64" | "x86_64" | "x64" => Some("x64".into()),
        "x86" | "i386" | "i686" | "x86_32" => Some("x86".into()),
        "aarch64" | "arm64" => Some("aarch64".into()),
        _ => None,
    }
}

/// 解析 `java -XshowSettings:properties -version` 的 stderr 输出
/// → (主版本号, 位数, 发行版)；任一关键属性缺失返回 None
pub fn parse_probe_output(text: &str) -> Option<(u32, String, String)> {
    let mut version = None;
    let mut arch = None;
    let mut vendor = String::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "java.version" => version = parse_java_version(value),
            "os.arch" => arch = normalize_arch(value),
            "java.vendor" => vendor = value.to_string(),
            _ => {}
        }
    }
    Some((version?, arch?, vendor))
}

/// 收集候选 java.exe 路径（已按真实存在过滤并去重）
pub fn collect_candidates(paths: &LauncherPaths) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    // 1. PATH 各目录
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            out.push(dir.join("java.exe"));
        }
    }
    // 2. JAVA_HOME
    if let Ok(home) = std::env::var("JAVA_HOME") {
        out.push(Path::new(&home).join("bin").join("java.exe"));
    }
    // 3. 注册表
    out.extend(registry_java_homes());
    // 4. Program Files 常见发行版目录
    for root in program_files_roots() {
        for vendor in [
            "Java",
            "Eclipse Adoptium",
            "Eclipse Foundation",
            "AdoptOpenJDK",
            "Microsoft",
            "Zulu",
            "Amazon Corretto",
            "BellSoft",
        ] {
            if let Ok(entries) = std::fs::read_dir(root.join(vendor)) {
                for e in entries.flatten() {
                    out.push(e.path().join("bin").join("java.exe"));
                }
            }
        }
    }
    // 5. 启动器自动下载目录：{root}/java/{major}/{子目录}/bin/java.exe（兼容两种布局）
    if let Ok(majors) = std::fs::read_dir(paths.java_dir()) {
        for m in majors.flatten() {
            let major_dir = m.path();
            out.push(major_dir.join("bin").join("java.exe"));
            if let Ok(subs) = std::fs::read_dir(&major_dir) {
                for s in subs.flatten() {
                    out.push(s.path().join("bin").join("java.exe"));
                }
            }
        }
    }
    // 6. %LOCALAPPDATA%\Programs（Adoptium / Temurin 用户级安装的默认位置，
    //    不进注册表；递归 3 层找 bin\java.exe）
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let programs = Path::new(&local).join("Programs");
        out.extend(walk_java_bins(&programs, 3));
    }
    // 7. 官方启动器自带运行时（跑过官方启动器的机器必有现成 Java）：
    //    新版 UWP 沙盒路径 java-runtime-delta + 老版 %LOCALAPPDATA%\minecraft\java-runtime*
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let local = Path::new(&local);
        out.extend(walk_java_bins(
            &local
                .join("Packages")
                .join("Microsoft.4297127D64EC6_8wekyb3d8bbwe")
                .join("LocalCache")
                .join("Local")
                .join("java-runtime-delta"),
            3,
        ));
        if let Ok(entries) = std::fs::read_dir(local.join("minecraft")) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with("java-runtime") {
                    out.extend(walk_java_bins(&e.path(), 3));
                }
            }
        }
    }
    // 8. 用户手动登记的路径（java.exe 或其所在目录）
    for custom in custom_java_paths(paths) {
        if custom.is_file() {
            out.push(custom);
        } else if custom.is_dir() {
            out.push(custom.join("bin").join("java.exe"));
            out.push(custom.join("java.exe"));
        }
    }

    // 过滤存在性并去重（canonicalize 归一大小写与符号链接）
    let mut seen = BTreeSet::new();
    out.into_iter()
        .filter(|p| p.is_file())
        .filter(|p| seen.insert(p.canonicalize().unwrap_or_else(|_| p.clone())))
        .collect()
}

/// 在 root 下递归（限深）查找 `bin\java.exe`。目录不存在返回空。
fn walk_java_bins(root: &Path, max_depth: u32) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            if p.file_name().map(|n| n == "bin").unwrap_or(false) {
                let java = p.join("java.exe");
                if java.is_file() {
                    out.push(java);
                }
            } else if depth + 1 < max_depth {
                stack.push((p, depth + 1));
            }
        }
    }
    out
}

/// 读取 settings.json 中用户手动登记的 Java 路径（同步轻量读取；
/// detect_all 在阻塞线程执行，直接读文件无碍）
fn custom_java_paths(paths: &LauncherPaths) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(paths.settings_path()) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    v.get("customJavaPaths")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// 注册表 JavaSoft 各版本子键的 JavaHome（64 位视图）
fn registry_java_homes() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let mut out = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for root_name in [
        "SOFTWARE\\JavaSoft\\JDK",
        "SOFTWARE\\JavaSoft\\JRE",
        "SOFTWARE\\JavaSoft\\Java Runtime Environment",
    ] {
        let Ok(root) = hklm.open_subkey_with_flags(root_name, KEY_READ) else {
            continue;
        };
        for ver in root.enum_keys().flatten() {
            let Ok(sub) = root.open_subkey(&ver) else {
                continue;
            };
            if let Ok(home) = sub.get_value::<String, _>("JavaHome") {
                out.push(Path::new(&home).join("bin").join("java.exe"));
            }
        }
    }
    out
}

/// Program Files 根目录（64 位与 32 位视图）
fn program_files_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(PathBuf::from)
        .collect()
}

/// 探测单个 java.exe（带超时：进程挂死时 kill 并返回 None）
fn probe_java_with_timeout(path: &Path) -> Option<JavaInfo> {
    let mut cmd = Command::new(path);
    cmd.arg("-XshowSettings:properties")
        .arg("-version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    // GUI 进程派生控制台程序时 Windows 会分配新控制台，检测 probe 必须隐藏黑窗
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // 进程已退出，取输出。按字节读再 lossy 解码：
                // JVM 重定向到管道的 stderr 用本机编码（中文 Windows = GBK），
                // 含非 ASCII（如中文安装路径的 java.home）时 UTF-8 严格解码会
                // 中途报错，丢掉排在后面的 java.version / os.arch 行
                let mut stderr = child.stderr.take()?;
                use std::io::Read;
                let mut bytes = Vec::new();
                let _ = stderr.read_to_end(&mut bytes);
                let text = String::from_utf8_lossy(&bytes).into_owned();
                if !status.success() && text.is_empty() {
                    return None;
                }
                let (version, arch, vendor) = parse_probe_output(&text)?;
                return Some(JavaInfo {
                    path: path.to_string_lossy().to_string(),
                    version,
                    arch,
                    vendor,
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

/// 全量检测：扫描候选并逐一探测
pub fn detect_all(paths: &LauncherPaths) -> Vec<JavaInfo> {
    collect_candidates(paths)
        .iter()
        .filter_map(|p| probe_java_with_timeout(p))
        .collect()
}

/// 探测单个 java.exe / 其所在目录（供手动登记入口校验路径有效性）：
/// 传入目录时自动尝试 {dir}\bin\java.exe 与 {dir}\java.exe
pub fn probe_java_path(path: &Path) -> Option<JavaInfo> {
    if path.is_file() {
        return probe_java_with_timeout(path);
    }
    if path.is_dir() {
        for cand in [path.join("bin").join("java.exe"), path.join("java.exe")] {
            if cand.is_file() {
                if let Some(info) = probe_java_with_timeout(&cand) {
                    return Some(info);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_java_bins_finds_nested_and_limits_depth() {
        let base = std::env::temp_dir().join(format!("piemcl-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        // 深度 2 处的 bin（Adoptium 布局：Programs\vendor\jdk-21\bin）
        let deep = base.join("vendor").join("jdk-21").join("bin");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("java.exe"), b"").unwrap();
        // 深度 4 处的 bin（超出 max_depth=3，不应命中）
        let too_deep = base.join("a").join("b").join("c").join("bin");
        std::fs::create_dir_all(&too_deep).unwrap();
        std::fs::write(too_deep.join("java.exe"), b"").unwrap();

        let found = walk_java_bins(&base, 3);
        assert_eq!(found.len(), 1, "只应命中限深内的 bin\\java.exe");
        assert!(found[0].ends_with(r"jdk-21\bin\java.exe"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn required_java_matrix() {
        for (mc, want) in [
            ("1.7.10", 8u32),
            ("1.8.9", 8),
            ("1.12.2", 8),
            ("1.16.5", 8),
            ("1.17", 17),
            ("1.17.1", 17),
            ("1.18", 17),
            ("1.18.2", 17),
            ("1.19.4", 17),
            ("1.20.1", 17),
            ("1.20.4", 17),
            ("1.20.5", 21),
            ("1.20.6", 21),
            ("1.21", 21),
            ("1.21.1", 21),
            ("1.21.4", 21),
            // 新版本号方案（年份式版本）
            ("26.1", 25),
            ("26.2", 25),
        ] {
            assert_eq!(required_java(mc), want, "MC {mc} 应要求 Java {want}");
        }
        // 非常规输入回落 8
        assert_eq!(required_java("rd-132211"), 8);
        assert_eq!(required_java("foo"), 8);
    }

    #[test]
    fn max_java_matrix() {
        // 老版本上限 8
        for mc in ["1.7.10", "1.8.9", "1.12.2", "1.16.5"] {
            assert_eq!(max_java(mc), Some(8), "MC {mc} 应有 Java 8 上限");
        }
        // 1.17+ 无上限
        for mc in ["1.17", "1.20.4", "1.21.4"] {
            assert_eq!(max_java(mc), None, "MC {mc} 应无 Java 上限");
        }
        // 新版本号方案（年份式版本）：无上限（此前误回落经典版兜底 Some(8)，
        // 导致 26.x「Java 25 太高 / Java 21 太低」左右互搏）
        for mc in ["26.1", "26.2"] {
            assert_eq!(max_java(mc), None, "MC {mc} 应无 Java 上限");
        }
        // 经典版兜底
        assert_eq!(max_java("rd-132211"), Some(8));
        assert_eq!(max_java("foo"), Some(8));
    }

    #[test]
    fn required_java_from_json_priority() {
        // 官方标注优先：无论版本号如何，以 javaVersion.majorVersion 为准
        let v = serde_json::json!({
            "id": "26.2",
            "javaVersion": { "component": "java-runtime-delta", "majorVersion": 25 }
        });
        assert_eq!(required_java_from_json(&v), Some(25));
        // 无 javaVersion 字段（老版本）→ None，调用方回落规则表
        let old = serde_json::json!({ "id": "1.8.9" });
        assert_eq!(required_java_from_json(&old), None);
        // 非法值 → None
        let bad = serde_json::json!({ "javaVersion": { "majorVersion": "abc" } });
        assert_eq!(required_java_from_json(&bad), None);
    }

    #[test]
    fn version_parsing() {
        assert_eq!(parse_java_version("21.0.5"), Some(21));
        assert_eq!(parse_java_version("17.0.2"), Some(17));
        assert_eq!(parse_java_version("1.8.0_401"), Some(8));
        assert_eq!(parse_java_version("1.7.0_80"), Some(7));
        assert_eq!(parse_java_version("8"), Some(8));
        assert_eq!(parse_java_version("abc"), None);
    }

    #[test]
    fn arch_normalization() {
        assert_eq!(normalize_arch("amd64").as_deref(), Some("x64"));
        assert_eq!(normalize_arch("x86_64").as_deref(), Some("x64"));
        assert_eq!(normalize_arch("x86").as_deref(), Some("x86"));
        assert_eq!(normalize_arch("aarch64").as_deref(), Some("aarch64"));
        assert_eq!(normalize_arch("arm64").as_deref(), Some("aarch64"));
        assert_eq!(normalize_arch("mips"), None);
    }

    #[test]
    fn probe_output_parsing() {
        // 模拟 Adoptium Java 21 在 Windows 上的实际输出（属性行有前导缩进）
        let sample = "\
openjdk version \"21.0.5\" 2024-10-15 LTS
OpenJDK Runtime Environment Temurin-21.0.5+11 (build 21.0.5+11-LTS)
OpenJDK 64-Bit Server VM Temurin-21.0.5+11 (build 21.0.5+11-LTS, mixed mode, sharing)

Property settings:
    file.encoding = UTF-8
    java.home = C:\\Program Files\\Eclipse Adoptium\\jdk-21.0.5+11-jre
    java.vendor = Temurin
    java.version = 21.0.5
    os.arch = amd64
    os.name = Windows 11

";
        let (v, arch, vendor) = parse_probe_output(sample).unwrap();
        assert_eq!(v, 21);
        assert_eq!(arch, "x64");
        assert_eq!(vendor, "Temurin");

        // Java 8 旧命名 + x86
        let old = "    java.version = 1.8.0_401\n    os.arch = x86\n    java.vendor = Oracle Corporation\n";
        let (v, arch, _) = parse_probe_output(old).unwrap();
        assert_eq!(v, 8);
        assert_eq!(arch, "x86");

        // 缺关键属性 → None
        assert!(parse_probe_output("    java.vendor = Foo\n").is_none());
    }
}
