// 启动命令构建
//
// 严格按 Mojang 规范组装 java 命令：
// JVM 参数（version.json arguments.jvm 条件过滤 + 内存 + 预设 + 用户追加）
// → mainClass → Game 参数（arguments.game 或老版 minecraftArguments）
// → 13 个标准占位符替换。
// Fabric/Forge 的 mainClass、追加参数与库已在阶段三合并进冻结 version.json，
// 此处直接使用，无需二次处理 loader.json。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use uuid::Uuid;

use crate::core::error::{AppError, AppResult};
use crate::instance::model::{preset_args, JvmSettings};
use crate::java::model::JavaInfo;

use super::rules::{rules_allow, LaunchFeatures};

/// 构建完成的启动计划
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    /// java 可执行文件路径
    pub program: PathBuf,
    /// 完整参数（含 mainClass，不含 program 自身）
    pub args: Vec<String>,
    /// 附加环境变量（预留，当前为空）
    pub envs: HashMap<String, String>,
    /// 工作目录 = 实例 minecraft 目录
    pub working_dir: PathBuf,
    /// 可读命令行（前端日志面板折叠展示）
    pub summary: String,
}

/// 认证参数上下文（离线 / 正版由账户层填充）
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// 游戏内 UUID
    pub uuid: String,
    /// access token（离线 = "0"）
    pub access_token: String,
    /// 正版 msa / 离线 legacy
    pub user_type: String,
    /// Xbox xuid（正版；离线为空）
    pub xuid: String,
}

impl AuthContext {
    /// 离线模式：UUID v3 派生，token 固定 "0"（官方离线行为）
    pub fn offline(player_name: &str) -> Self {
        Self {
            uuid: offline_uuid(player_name).to_string(),
            access_token: "0".into(),
            user_type: "legacy".into(),
            xuid: String::new(),
        }
    }
}

/// 构建命令所需的上下文
pub struct LaunchContext<'a> {
    pub version_json: &'a Value,
    /// 游戏公共根（.minecraft）：classpath 库、assets、versions 主 jar
    pub game_root: &'a Path,
    /// 游戏工作目录（${game_directory}，隔离开启 = versions/{实例名}，关闭 = .minecraft 根）
    pub game_dir: &'a Path,
    /// natives 解压目录
    pub natives_dir: &'a Path,
    pub java: &'a JavaInfo,
    pub jvm: &'a JvmSettings,
    /// 玩家名（离线 = 自定义 ID；正版 = profile name）
    pub player_name: &'a str,
    /// 认证参数（UUID / token / user_type / xuid）
    pub auth: &'a AuthContext,
    /// 启动器注入的附加 JVM 参数（如离线皮肤的 -javaagent）
    pub extra_jvm: &'a [String],
    /// 窗口分辨率（全屏=显示器分辨率，自定义=用户设置；None = 未启用 resolution feature）
    pub resolution: Option<(u32, u32)>,
    /// 启动后直连的服务器（None = 不直连）；追加 --server/--port 游戏参数
    pub server: Option<(String, u16)>,
}

/// 离线模式 UUID：UUID v3（MD5，nil namespace），与官方离线算法一致
pub fn offline_uuid(player_name: &str) -> Uuid {
    let data = format!("OfflinePlayer:{player_name}");
    Uuid::new_v3(&Uuid::nil(), data.as_bytes())
}

/// 从检测列表中选择启动用 Java：满足下限 + 不超上限（老版本 MC 需要旧 Java）+
/// x64，取主版本最接近要求者
pub fn select_java<'a>(
    candidates: &'a [JavaInfo],
    required_major: u32,
    max_major: Option<u32>,
) -> Option<&'a JavaInfo> {
    candidates
        .iter()
        .filter(|j| {
            j.arch == "x64"
                && j.version >= required_major
                && max_major.map(|m| j.version <= m).unwrap_or(true)
        })
        .min_by_key(|j| j.version)
}

/// 组装完整 java 命令
pub fn build_command(ctx: &LaunchContext) -> AppResult<LaunchPlan> {
    let v = ctx.version_json;
    let mc_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AppError::msg("version.json 缺少 id 字段"))?;
    let main_class = v
        .get("mainClass")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AppError::msg("version.json 缺少 mainClass 字段"))?
        .to_string();

    let features = LaunchFeatures {
        has_custom_resolution: ctx.resolution.is_some(),
    };

    // -----------------------------------------------------------------
    // classpath：主 jar + 按 version.json 顺序的全部 artifact（; 分隔）
    // -----------------------------------------------------------------
    let classpath = build_classpath(v, ctx.game_root)?;

    // -----------------------------------------------------------------
    // 占位符表
    // -----------------------------------------------------------------
    let assets_index = v
        .pointer("/assetIndex/id")
        .and_then(|x| x.as_str())
        .unwrap_or(mc_id);
    let version_type = v.get("type").and_then(|x| x.as_str()).unwrap_or("release");
    let mut placeholders: HashMap<String, String> = HashMap::new();
    placeholders.insert("natives_directory".into(), ctx.natives_dir.to_string_lossy().into());
    placeholders.insert("launcher_name".into(), "PieMCL".into());
    placeholders.insert("launcher_version".into(), env!("CARGO_PKG_VERSION").into());
    placeholders.insert("classpath".into(), classpath.clone());
    placeholders.insert("auth_player_name".into(), ctx.player_name.into());
    placeholders.insert("version_name".into(), mc_id.into());
    placeholders.insert("game_directory".into(), ctx.game_dir.to_string_lossy().into());
    placeholders.insert("assets_root".into(), ctx.game_root.join("assets").to_string_lossy().into());
    placeholders.insert("assets_index_name".into(), assets_index.into());
    placeholders.insert("auth_uuid".into(), ctx.auth.uuid.clone());
    placeholders.insert("auth_access_token".into(), ctx.auth.access_token.clone());
    placeholders.insert("user_type".into(), ctx.auth.user_type.clone());
    // 1.8.9 等老版 minecraftArguments 含 --userProperties ${user_properties}，官方值为空 JSON 对象
    placeholders.insert("user_properties".into(), "{}".into());
    // clientId 启动器暂不注册（传空）；xuid 正版账户携带
    placeholders.insert("clientid".into(), String::new());
    placeholders.insert("auth_xuid".into(), ctx.auth.xuid.clone());
    placeholders.insert("auth_session".into(), String::new());
    placeholders.insert("version_type".into(), version_type.into());
    if let Some((w, h)) = ctx.resolution {
        placeholders.insert("resolution_width".into(), w.to_string());
        placeholders.insert("resolution_height".into(), h.to_string());
    }
    // 兼容部分第三方 version.json 的额外占位符
    placeholders.insert("classpath_separator".into(), ";".into());
    placeholders.insert("library_directory".into(), ctx.game_root.join("libraries").to_string_lossy().into());
    placeholders.insert("cpu_arch".into(), "${cpu_arch}".into()); // Forge 专用占位原样透传，由其自行处理

    // -----------------------------------------------------------------
    // JVM 参数
    // -----------------------------------------------------------------
    let mut jvm: Vec<String> = Vec::new();
    // 启动器注入参数（离线皮肤 -javaagent 等）放最前，不与版本 / 用户参数冲突
    jvm.extend(ctx.extra_jvm.iter().cloned());
    if let Some(arr) = v.pointer("/arguments/jvm").and_then(|x| x.as_array()) {
        // 条件规则的 JVM 参数（1.13+）
        for item in arr {
            if let Some(s) = item.as_str() {
                jvm.push(s.to_string());
            } else if rules_allow(item.get("rules"), &features) {
                // 条件对象：value 可能是字符串或数组
                match item.get("value") {
                    Some(Value::String(s)) => jvm.push(s.clone()),
                    Some(Value::Array(arr)) => {
                        for s in arr {
                            if let Some(s) = s.as_str() {
                                jvm.push(s.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    } else {
        // 老版本（1.12- 无 arguments.jvm）：JVM 参数由启动器自行拼接
        // （新式版本的 -cp 与 -Djava.library.path 内置于 arguments.jvm）
        jvm.push(format!(
            "-Djava.library.path={}",
            ctx.natives_dir.to_string_lossy()
        ));
        jvm.push("-cp".into());
        jvm.push(classpath.clone());
        // 旧版 Forge（launchwrapper + tweakClass）：binpatch 应用后与官方签名存在
        // 差异，FML 要求容忍证书不一致（官方启动器 / HMCL / PCL 均同样处理）
        if v.get("minecraftArguments")
            .and_then(|x| x.as_str())
            .map(|s| s.contains("tweakClass"))
            .unwrap_or(false)
        {
            jvm.push("-Dfml.ignoreInvalidMinecraftCertificates=true".into());
            jvm.push("-Dfml.ignorePatchDiscrepancies=true".into());
        }
    }
    // 用户内存参数（在条件参数之后注入，可覆盖其内置 -Xmx 类参数）
    if ctx.jvm.min_memory_mb > 0 {
        jvm.push(format!("-Xms{}M", ctx.jvm.min_memory_mb));
    }
    jvm.push(format!("-Xmx{}M", ctx.jvm.max_memory_mb));
    // 优化预设 + 用户追加（放最后，可覆盖同名参数）
    jvm.extend(preset_args(&ctx.jvm.preset).iter().map(|s| s.to_string()));
    for s in ctx.jvm.extra_args.split_whitespace() {
        jvm.push(s.to_string());
    }

    // -----------------------------------------------------------------
    // Game 参数（1.13+ 条件式 或 老版 minecraftArguments）
    // -----------------------------------------------------------------
    let mut game: Vec<String> = Vec::new();
    if let Some(arr) = v.pointer("/arguments/game").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(s) = item.as_str() {
                game.push(s.to_string());
            } else if rules_allow(item.get("rules"), &features) {
                match item.get("value") {
                    Some(Value::String(s)) => game.push(s.clone()),
                    Some(Value::Array(arr)) => {
                        for s in arr {
                            if let Some(s) = s.as_str() {
                                game.push(s.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    } else if let Some(margs) = v.get("minecraftArguments").and_then(|x| x.as_str()) {
        game.extend(margs.split(' ').filter(|s| !s.is_empty()).map(str::to_string));
    }

    // -----------------------------------------------------------------
    // 占位符替换（迭代两轮处理嵌套引用）
    // -----------------------------------------------------------------
    let replace_all = |input: String, ph: &HashMap<String, String>| -> String {
        let mut out = input;
        for _ in 0..2 {
            let mut changed = false;
            for (k, val) in ph {
                let token = format!("${{{k}}}");
                if out.contains(&token) {
                    out = out.replace(&token, val);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        out
    };
    let jvm: Vec<String> = jvm.into_iter().map(|s| replace_all(s, &placeholders)).collect();
    let mut game: Vec<String> = game.into_iter().map(|s| replace_all(s, &placeholders)).collect();

    // 直连服务器：追加 --server/--port 游戏参数（1.7+ 游戏内置支持，
    // 覆盖 options.txt 的 server 字段；老版 minecraftArguments 版本同样被解析）
    if let Some((host, port)) = &ctx.server {
        game.push("--server".into());
        game.push(host.clone());
        game.push("--port".into());
        game.push(port.to_string());
    }

    // -----------------------------------------------------------------
    // 组装：java [jvm...] mainClass [game...]
    // -----------------------------------------------------------------
    let mut args = jvm;
    args.push(main_class);
    args.extend(game);

    let summary = format!("{} {}", ctx.java.path, args.join(" "));

    Ok(LaunchPlan {
        program: PathBuf::from(&ctx.java.path),
        args,
        envs: HashMap::new(),
        working_dir: ctx.game_dir.to_path_buf(),
        summary,
    })
}

/// classpath：主 jar + 全部 artifact（保留 natives jar 在 classpath 上：
/// 1.19+ 由 LWJGL3 自行提取；1.18- 的 natives 已解压且 jar 同时保留无害）
/// 主 jar 位置与资源补全一致：versions/{id}/{id}.jar
/// （1.8.9 等老版本的 downloads.client 无 path 字段，不能依赖它定位）
fn build_classpath(v: &Value, game_root: &Path) -> AppResult<String> {
    let mc_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AppError::msg("version.json 缺少 id 字段"))?;
    let mut parts: Vec<PathBuf> =
        vec![game_root.join("versions").join(mc_id).join(format!("{mc_id}.jar"))];

    if let Some(libs) = v.get("libraries").and_then(|x| x.as_array()) {
        for lib in libs {
            // classpath 不做 rules 过滤：Mojang 规范将全部 artifact 上 classpath，
            // 平台相关 natives jar 由启动器按需解压/忽略
            if let Some(path) = lib.pointer("/downloads/artifact/path").and_then(|x| x.as_str()) {
                parts.push(game_root.join("libraries").join(path));
            } else if let Some(rel) = lib
                .get("name")
                .and_then(|x| x.as_str())
                .and_then(crate::game::planner::maven_path_from_name)
            {
                // Fabric / Quilt 风格：仅 Maven 坐标 + 仓库基址（无 downloads.artifact），
                // 按坐标推导本地路径（与 planner 的下载规划同规则）
                parts.push(game_root.join("libraries").join(rel));
            }
        }
    }
    Ok(parts
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    /// 1.20.1 风格 fixture（现代 arguments 条件式）
    fn fixture_modern() -> Value {
        json!({
            "id": "1.20.1",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assetIndex": { "id": "5" },
            "arguments": {
                "game": [
                    "--username", "${auth_player_name}",
                    "--version", "${version_name}",
                    "--assetsDir", "${assets_root}",
                    "--assetIndex", "${assets_index_name}",
                    "--versionType", "${version_type}",
                    "--clientId", "${clientid}",
                    "--xuid", "${auth_xuid}",
                    { "rules": [{ "action": "allow", "features": { "has_custom_resolution": true } }],
                      "value": ["--width", "${resolution_width}", "--height", "${resolution_height}"] }
                ],
                "jvm": [
                    "-Djava.library.path=${natives_directory}",
                    "-cp",
                    "${classpath}",
                    { "rules": [{ "action": "allow", "os": { "name": "windows" } }],
                      "value": "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe" }
                ]
            },
            "downloads": { "client": { "path": "com/mojang/minecraft/1.20.1/client-1.20.1.jar" } },
            "libraries": [
                { "name": "com.mojang:brigadier:1.0",
                  "downloads": { "artifact": { "path": "com/mojang/brigadier/1.0/brigadier-1.0.jar" } } },
                { "name": "org.lwjgl:lwjgl:3.3.1:natives-windows",
                  "downloads": { "artifact": { "path": "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar" } } }
            ]
        })
    }

    /// 1.7.10 风格 fixture（老版 minecraftArguments，无 arguments.jvm）
    fn fixture_legacy() -> Value {
        json!({
            "id": "1.7.10",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "minecraftArguments": "--username ${auth_player_name} --version ${version_name} --gameDir ${game_directory} --assetsDir ${assets_root} --userProperties ${user_properties}",
            "downloads": { "client": { "path": "1.7.10/client.jar" } },
            "libraries": []
        })
    }

    fn java() -> JavaInfo {
        JavaInfo {
            path: r"C:\java\bin\java.exe".into(),
            version: 17,
            arch: "x64".into(),
            vendor: "Temurin".into(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ctx_with_server<'a>(
        v: &'a Value,
        jvm: &'a JvmSettings,
        natives: &'a Path,
        resolution: Option<(u32, u32)>,
        java: &'a JavaInfo,
        auth: &'a AuthContext,
        extra_jvm: &'a [String],
        server: Option<(String, u16)>,
    ) -> LaunchContext<'a> {
        LaunchContext {
            version_json: v,
            game_root: Path::new("C:/mc"),
            game_dir: Path::new("C:/mc/versions/test"),
            natives_dir: natives,
            java,
            jvm,
            player_name: "Steve",
            auth,
            extra_jvm,
            resolution,
            server,
        }
    }

    fn ctx<'a>(
        v: &'a Value,
        jvm: &'a JvmSettings,
        natives: &'a Path,
        resolution: Option<(u32, u32)>,
        java: &'a JavaInfo,
        auth: &'a AuthContext,
        extra_jvm: &'a [String],
    ) -> LaunchContext<'a> {
        ctx_with_server(v, jvm, natives, resolution, java, auth, extra_jvm, None)
    }

    /// 直连服务器：--server/--port 追加在游戏参数末尾
    #[test]
    fn server_direct_connect_args_appended() {
        let v = json!({ "id": "1.20.1", "mainClass": "net.minecraft.client.main.Main" });
        let jvm = JvmSettings::default();
        let natives = PathBuf::from("C:/mc/natives");
        let j = java();
        let auth = AuthContext::offline("Steve");
        let plan = build_command(&ctx_with_server(
            &v,
            &jvm,
            &natives,
            None,
            &j,
            &auth,
            &[],
            Some(("mc.example.com".into(), 25565)),
        ))
        .unwrap();
        let pos = plan
            .args
            .iter()
            .position(|a| a == "--server")
            .expect("应有 --server 参数");
        assert_eq!(plan.args[pos + 1], "mc.example.com");
        assert_eq!(plan.args[pos + 2], "--port");
        assert_eq!(plan.args[pos + 3], "25565");
        // 必须在 mainClass 之后（游戏参数区）
        let mc = plan.args.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        assert!(pos > mc);
        // 不传 server 时不应出现
        let plan2 = build_command(&ctx(&v, &jvm, &natives, None, &j, &auth, &[])).unwrap();
        assert!(!plan2.args.contains(&"--server".to_string()));
    }

    #[test]
    fn auth_context_offline_and_msa() {
        // 离线：token "0" + legacy + 空 xuid + UUID 与 offline_uuid 一致
        let a = AuthContext::offline("Steve");
        assert_eq!(a.access_token, "0");
        assert_eq!(a.user_type, "legacy");
        assert_eq!(a.xuid, "");
        assert_eq!(a.uuid, offline_uuid("Steve").to_string());

        // 正版：msa + 真实 token + xuid
        let m = AuthContext {
            uuid: "abc".into(),
            access_token: "tok".into(),
            user_type: "msa".into(),
            xuid: "2535".into(),
        };
        assert_eq!(m.user_type, "msa");
    }

    #[test]
    fn offline_uuid_deterministic() {
        let a = offline_uuid("Steve");
        let b = offline_uuid("Steve");
        assert_eq!(a, b, "同名玩家 UUID 必须确定");
        assert_ne!(offline_uuid("Alex"), a);
        let s = a.to_string();
        assert_eq!(s.len(), 36, "UUID 格式 8-4-4-4-12");
        // v3 版本位（MD5 派生，与官方 nameUUIDFromBytes 一致）
        assert_eq!(&s[14..15], "3");
        assert!(matches!(s.chars().nth(19), Some('8' | '9' | 'a' | 'b')), "IETF variant 位");
    }

    #[test]
    fn build_modern_command() {
        let v = fixture_modern();
        let mut jvm = JvmSettings::default();
        jvm.min_memory_mb = 1024;
        jvm.max_memory_mb = 4096;
        jvm.preset = "low_spec".into();
        jvm.extra_args = "-XX:+UseZGC".into();
        let natives = PathBuf::from("C:/mc/natives");
        let j = java();
        let auth = AuthContext::offline("Steve");
        let plan =
            build_command(&ctx(&v, &jvm, &natives, Some((1280, 720)), &j, &auth, &[])).unwrap();

        // JVM 段
        let lib = plan
            .args
            .iter()
            .find(|a| a.starts_with("-Djava.library.path="))
            .unwrap();
        assert_eq!(lib, "-Djava.library.path=C:/mc/natives");
        assert!(plan.args.iter().any(|a| a == "-Xms1024M"));
        assert!(plan.args.iter().any(|a| a == "-Xmx4096M"));
        assert!(plan.args.iter().any(|a| a == "-XX:G1NewSizePercent=20"), "low_spec 预设");
        // 用户追加在预设之后
        let zgc = plan.args.iter().position(|a| a == "-XX:+UseZGC").unwrap();
        let g1 = plan.args.iter().position(|a| a == "-XX:G1NewSizePercent=20").unwrap();
        assert!(zgc > g1);
        // windows 条件参数
        assert!(plan.args.iter().any(|a| a.contains("HeapDumpPath")));
        // 分辨率 feature 注入
        let w = plan.args.iter().position(|a| a == "--width").unwrap();
        assert_eq!(plan.args[w + 1], "1280");
        // mainClass 位置：jvm 之后、game 之前
        let mc = plan.args.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        assert!(mc > 0);
        assert_eq!(plan.args[mc + 1], "--username");
        assert_eq!(plan.args[mc + 2], "Steve");
        // 占位符
        assert!(plan.args.iter().any(|a| a == "1.20.1"), "version_name");
        assert!(plan.args.iter().any(|a| a == "release"), "version_type");
        assert!(plan.args.iter().any(|a| a == "5"), "assets_index_name");
        // clientId / xuid 离线模式替换为空值
        let cid = plan.args.iter().position(|a| a == "--clientId").unwrap();
        assert_eq!(plan.args[cid + 1], "");
        let xuid = plan.args.iter().position(|a| a == "--xuid").unwrap();
        assert_eq!(plan.args[xuid + 1], "");
        // classpath 含主 jar 与两个库，分号分隔（主 jar 固定 versions/{id}/{id}.jar，不依赖 client.path）
        let cp = plan.args.iter().find(|a| !a.starts_with('-') && a.contains(';')).unwrap();
        assert!(
            cp.contains("versions\\1.20.1\\1.20.1.jar") || cp.contains("versions/1.20.1/1.20.1.jar")
        );
        assert!(cp.contains("brigadier-1.0.jar"));
        assert!(cp.contains("natives-windows.jar"), "natives artifact 也在 classpath");
        // program 与工作目录
        assert_eq!(plan.program, PathBuf::from(r"C:\java\bin\java.exe"));
        assert_eq!(plan.working_dir, PathBuf::from("C:/mc/versions/test"));
    }

    /// Fabric / Quilt 风格库（仅 Maven 坐标，无 downloads.artifact）也须进 classpath
    #[test]
    fn classpath_includes_coordinate_only_libraries() {
        let v = json!({
            "id": "1.16.5",
            "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries": [
                { "name": "net.fabricmc:fabric-loader:0.19.4", "url": "https://maven.fabricmc.net/" },
                { "name": "net.fabricmc:intermediary:1.16.5", "url": "https://maven.fabricmc.net/" }
            ]
        });
        let cp = build_classpath(&v, Path::new("C:/mc")).unwrap();
        assert!(
            cp.contains("net/fabricmc/fabric-loader/0.19.4/fabric-loader-0.19.4.jar"),
            "坐标推导的 loader jar 应在 classpath：{cp}"
        );
        assert!(
            cp.contains("net/fabricmc/intermediary/1.16.5/intermediary-1.16.5.jar"),
            "坐标推导的 intermediary jar 应在 classpath：{cp}"
        );
    }

    #[test]
    fn build_without_resolution_omits_feature_args() {
        let v = fixture_modern();
        let jvm = JvmSettings::default();
        let natives = PathBuf::from("C:/mc/natives");
        let j = java();
        let auth = AuthContext::offline("Steve");
        let plan = build_command(&ctx(&v, &jvm, &natives, None, &j, &auth, &[])).unwrap();
        assert!(!plan.args.contains(&"--width".to_string()));
        // min=0 → 无 -Xms
        assert!(!plan.args.iter().any(|a| a.starts_with("-Xms")));
        // default 预设无附加
        assert!(!plan.args.iter().any(|a| a == "-XX:MaxGCPauseMillis=50"));
    }

    #[test]
    fn build_legacy_minecraft_arguments() {
        let v = fixture_legacy();
        let jvm = JvmSettings::default();
        let natives = PathBuf::from("C:/mc/natives");
        let j = java();
        let auth = AuthContext::offline("Steve");
        let plan = build_command(&ctx(&v, &jvm, &natives, None, &j, &auth, &[])).unwrap();
        let mc = plan.args.iter().position(|a| a == "net.minecraft.client.main.Main").unwrap();
        assert_eq!(plan.args[mc + 1], "--username");
        assert!(plan.args.contains(&"Steve".to_string()));
        assert!(plan.args.contains(&"1.7.10".to_string()));
        assert!(
            plan.args.iter().any(|a| a == "C:/mc/versions/test"),
            "game_directory 占位符"
        );
        // 老版本 JVM 参数由启动器补齐：natives 路径 + classpath
        assert!(
            plan.args.iter().any(|a| a == "-Djava.library.path=C:/mc/natives"),
            "老版本必须自带 -Djava.library.path"
        );
        let cp_pos = plan.args.iter().position(|a| a == "-cp").unwrap();
        assert!(cp_pos > 0 && cp_pos < mc, "-cp 在 mainClass 之前");
        let cp = &plan.args[cp_pos + 1];
        assert!(cp.contains("versions\\1.7.10\\1.7.10.jar") || cp.contains("versions/1.7.10/1.7.10.jar"));
        // userProperties 占位符替换为空 JSON 对象
        let up = plan.args.iter().position(|a| a == "--userProperties").unwrap();
        assert_eq!(plan.args[up + 1], "{}");
    }

    #[test]
    fn extra_jvm_injected_first() {
        let v = fixture_modern();
        let jvm = JvmSettings::default();
        let natives = PathBuf::from("C:/mc/natives");
        let j = java();
        let auth = AuthContext::offline("Steve");
        let extra = vec!["-javaagent:C:/agent.jar=http://127.0.0.1:12345".to_string()];
        let plan = build_command(&ctx(&v, &jvm, &natives, None, &j, &auth, &extra)).unwrap();
        // javaagent 位于 JVM 段（mainClass 之前）
        let agent = plan
            .args
            .iter()
            .position(|a| a.starts_with("-javaagent:"))
            .expect("javaagent 参数缺失");
        let mc = plan
            .args
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .unwrap();
        assert!(agent < mc);
        assert!(plan.args[agent].ends_with("=http://127.0.0.1:12345"));
    }

    #[test]
    fn select_java_prefers_closest() {
        let mk = |v: u32, arch: &str| JavaInfo {
            path: format!("C:/java{v}/bin/java.exe"),
            version: v,
            arch: arch.into(),
            vendor: "test".into(),
        };
        let list = vec![mk(8, "x64"), mk(17, "x64"), mk(21, "x64"), mk(17, "x86")];
        // 要求 17：在满足者中选最接近的 17（而非 21），x86 被过滤
        assert_eq!(select_java(&list, 17, None).unwrap().version, 17);
        // 要求 8：最小满足者
        assert_eq!(select_java(&list, 8, None).unwrap().version, 8);
        // 要求 22：无满足者
        assert!(select_java(&list, 22, None).is_none());
        // 上限 8（1.16.5 等老版本）：只能选 8，17/21 被过滤
        assert_eq!(select_java(&list, 8, Some(8)).unwrap().version, 8);
        assert!(select_java(&list, 17, Some(8)).is_none());
    }
}
