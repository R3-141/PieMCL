// version.json 继承链（inheritsFrom）解析与字段合并
//
// 合并规则（自最老祖先向下折叠，子覆盖父）：
// - mainClass / assetIndex / assets / javaVersion / logging / downloads / type：子覆盖父
// - minecraftArguments：子存在则整体覆盖
// - arguments.game / arguments.jvm：父在前、子追加，按值精确去重保序
// - libraries：子在前；父中 group:artifact 与子重复者剔除（子的坐标版本优先）
// - id 保留最子级；最终结果移除 inheritsFrom
//
// 纯合并逻辑与 IO 分离：merge_version_json / resolve_with 可离线测试；
// fetch_version_resolved 负责异步拉取链上所有版本 JSON 后折叠。

use serde_json::{Map, Value};

use crate::core::error::{AppError, AppResult};
use super::repo::VersionRepo;

/// 继承链最大深度
const MAX_INHERIT_DEPTH: usize = 8;

/// libraries 去重键：Maven 坐标的 group:artifact 段
fn library_key(lib: &Value) -> Option<String> {
    let name = lib.get("name")?.as_str()?;
    let mut parts = name.split(':');
    let group = parts.next()?;
    let artifact = parts.next()?;
    Some(format!("{group}:{artifact}"))
}

/// 合并 arguments 的 game / jvm 数组：父在前、子追加。
/// 仅跨版本去重：父内部去重，子只跳过与父完全相同的公共条目；
/// 子内部的重复条目必须保留（Forge 官方 version.json 的 jvm 数组会连续出现
/// 多个 "--add-opens" / "--add-exports" flag，若全局去重会删掉第二个 flag，
/// 其 value 被 JVM 当作主类导致启动失败 ClassNotFoundException）。
fn merge_arguments(parent: Option<&Value>, child: Option<&Value>) -> Option<Value> {
    if parent.is_none() && child.is_none() {
        return None;
    }
    let mut result = Map::new();
    for key in ["game", "jvm"] {
        let mut merged: Vec<Value> = Vec::new();
        let mut parent_seen: Vec<Value> = Vec::new();
        if let Some(arr) = parent.and_then(|p| p.get(key)).and_then(|v| v.as_array()) {
            for item in arr {
                if !parent_seen.contains(item) {
                    parent_seen.push(item.clone());
                    merged.push(item.clone());
                }
            }
        }
        if let Some(arr) = child.and_then(|c| c.get(key)).and_then(|v| v.as_array()) {
            for item in arr {
                // 与父重复的公共参数（如 -Djava.library.path）跳过；子内部重复保留
                if !parent_seen.contains(item) {
                    merged.push(item.clone());
                }
            }
        }
        if !merged.is_empty() {
            result.insert(key.to_string(), Value::Array(merged));
        }
    }
    Some(Value::Object(result))
}

/// 合并 libraries：子在前，父中与子同 group:artifact 的条目剔除
fn merge_libraries(parent: Option<&Value>, child: Option<&Value>) -> Option<Value> {
    let child_libs: Vec<Value> = child
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let parent_libs: Vec<Value> = parent
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if child_libs.is_empty() && parent_libs.is_empty() {
        return None;
    }
    let child_keys: Vec<String> = child_libs.iter().filter_map(library_key).collect();
    let mut merged = child_libs;
    for lib in parent_libs {
        match library_key(&lib) {
            Some(key) if child_keys.contains(&key) => continue,
            _ => merged.push(lib),
        }
    }
    Some(Value::Array(merged))
}

/// 子覆盖父的标量字段
const CHILD_OVERRIDE_KEYS: &[&str] = &[
    "mainClass",
    "assetIndex",
    "assets",
    "javaVersion",
    "logging",
    "downloads",
    "type",
    "minimumLauncherVersion",
    "releaseTime",
    "time",
];

/// 合并父版本（已解析）与子版本（原始）→ 新的合并结果
pub fn merge_version_json(parent: &Value, child: &Value) -> Value {
    let mut out = Map::new();

    // 1. 子覆盖字段：先父后子，子存在即覆盖
    for key in CHILD_OVERRIDE_KEYS {
        if let Some(v) = parent.get(*key) {
            out.insert((*key).to_string(), v.clone());
        }
        if let Some(v) = child.get(*key) {
            out.insert((*key).to_string(), v.clone());
        }
    }

    // 2. minecraftArguments：子整体覆盖
    if let Some(v) = parent.get("minecraftArguments") {
        out.insert("minecraftArguments".to_string(), v.clone());
    }
    if let Some(v) = child.get("minecraftArguments") {
        out.insert("minecraftArguments".to_string(), v.clone());
    }

    // 3. arguments：父 ++ 子去重
    if let Some(v) = merge_arguments(parent.get("arguments"), child.get("arguments")) {
        out.insert("arguments".to_string(), v);
    }

    // 4. libraries：子在前，父去重
    if let Some(v) = merge_libraries(parent.get("libraries"), child.get("libraries")) {
        out.insert("libraries".to_string(), v);
    }

    // 5. id 取最子级
    if let Some(v) = child.get("id").or_else(|| parent.get("id")) {
        out.insert("id".to_string(), v.clone());
    }

    Value::Object(out)
}

/// 纯逻辑解析：给定按 id 取版本 JSON 的闭包，解析继承链并合并
/// （当前由单元测试使用；加载器安装阶段解析 Forge 提供的 version.json 时复用）
#[allow(dead_code)]
pub fn resolve_with<F>(root_id: &str, fetcher: &F) -> AppResult<Value>
where
    F: Fn(&str) -> AppResult<Value>,
{
    let mut chain: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut current = root_id.to_string();

    loop {
        if seen.contains(&current) {
            return Err(AppError::msg(format!(
                "版本继承链存在循环：{} → {current}",
                seen.join(" → ")
            )));
        }
        if chain.len() >= MAX_INHERIT_DEPTH {
            return Err(AppError::msg(format!(
                "版本继承链超过最大深度 {MAX_INHERIT_DEPTH}：{}",
                seen.join(" → ")
            )));
        }
        seen.push(current.clone());
        let json = fetcher(&current)?;
        let parent_id = json
            .get("inheritsFrom")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        chain.push(json);
        match parent_id {
            Some(p) => current = p,
            None => break,
        }
    }

    // chain[0] 最子级，末尾是最老祖先；自祖先向下折叠
    let mut iter = chain.into_iter().rev();
    let mut acc = iter.next().expect("链非空");
    for child in iter {
        acc = merge_version_json(&acc, &child);
    }
    if let Some(obj) = acc.as_object_mut() {
        obj.remove("inheritsFrom");
    }
    Ok(acc)
}

/// 异步解析：从仓库拉取指定版本及其整条继承链，返回冻结的合并结果
pub async fn fetch_version_resolved(repo: &VersionRepo, id: &str) -> AppResult<Value> {
    let mut chain: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut current = id.to_string();

    loop {
        if seen.contains(&current) {
            return Err(AppError::msg(format!(
                "版本继承链存在循环：{} → {current}",
                seen.join(" → ")
            )));
        }
        if chain.len() >= MAX_INHERIT_DEPTH {
            return Err(AppError::msg(format!(
                "版本继承链超过最大深度 {MAX_INHERIT_DEPTH}：{}",
                seen.join(" → ")
            )));
        }
        seen.push(current.clone());
        let json = repo.fetch_version_json_by_id(&current).await?;
        let parent_id = json
            .get("inheritsFrom")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        chain.push(json);
        match parent_id {
            Some(p) => current = p,
            None => break,
        }
    }

    let mut iter = chain.into_iter().rev();
    let mut acc = iter.next().expect("链非空");
    for child in iter {
        acc = merge_version_json(&acc, &child);
    }
    if let Some(obj) = acc.as_object_mut() {
        obj.remove("inheritsFrom");
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    /// Forge 1.20.1 风格 fixture：子版本带 mainClass / 追加 arguments / 升级 lwjgl
    fn forge_fixture() -> HashMap<String, Value> {
        let parent = json!({
            "id": "1.20.1",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assetIndex": { "id": "5", "url": "https://piston-meta.mojang.com/v1/packages/xxx/5.json" },
            "assets": "5",
            "javaVersion": { "component": "java-runtime-delta", "majorVersion": 17 },
            "arguments": {
                "game": ["--username", "${auth_player_name}", "--version", "${version_name}"],
                "jvm": ["-Djava.library.path=${natives_directory}", "-cp", "${classpath}"]
            },
            "libraries": [
                { "name": "com.mojang:brigadier:1.2.9" },
                { "name": "org.lwjgl:lwjgl:3.3.1" }
            ]
        });
        let child = json!({
            "id": "1.20.1-forge-47.2.0",
            "inheritsFrom": "1.20.1",
            "type": "release",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "arguments": {
                "game": ["--launchTarget", "forgeclient"],
                "jvm": ["-Djava.library.path=${natives_directory}", "-p", "${library_directory}/cpw/mods/modulepath"]
            },
            "libraries": [
                { "name": "net.minecraftforge:forge:1.20.1-47.2.0" },
                { "name": "org.lwjgl:lwjgl:3.3.2" }
            ]
        });
        HashMap::from([("1.20.1".to_string(), parent), (child["id"].as_str().unwrap().to_string(), child)])
    }

    #[test]
    fn forge_inherits_merge() {
        let fixtures = forge_fixture();
        let fetcher = |id: &str| {
            fixtures
                .get(id)
                .cloned()
                .ok_or_else(|| AppError::msg(format!("fixture 缺少版本 {id}")))
        };
        let resolved = resolve_with("1.20.1-forge-47.2.0", &fetcher).expect("解析成功");

        // id 保留最子级；inheritsFrom 移除
        assert_eq!(resolved["id"], "1.20.1-forge-47.2.0");
        assert!(resolved.get("inheritsFrom").is_none());
        // mainClass 子覆盖父
        assert_eq!(resolved["mainClass"], "cpw.mods.bootstraplauncher.BootstrapLauncher");
        // assetIndex / assets / javaVersion 继承自父
        assert_eq!(resolved["assetIndex"]["id"], "5");
        assert_eq!(resolved["assets"], "5");
        assert_eq!(resolved["javaVersion"]["majorVersion"], 17);

        // arguments.game：父在前、子追加、无重复
        let game = resolved["arguments"]["game"].as_array().unwrap();
        assert_eq!(
            game.iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(),
            vec!["--username", "${auth_player_name}", "--version", "${version_name}", "--launchTarget", "forgeclient"]
        );
        // arguments.jvm：重复项 "-Djava.library.path=${natives_directory}" 只保留一份
        let jvm = resolved["arguments"]["jvm"].as_array().unwrap();
        assert_eq!(
            jvm.iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>(),
            vec!["-Djava.library.path=${natives_directory}", "-cp", "${classpath}", "-p", "${library_directory}/cpw/mods/modulepath"]
        );

        // libraries：子的 forge 在最前；org.lwjgl:lwjgl 只出现一次且为子的 3.3.2
        let libs = resolved["libraries"].as_array().unwrap();
        assert_eq!(libs[0]["name"], "net.minecraftforge:forge:1.20.1-47.2.0");
        let lwjgl: Vec<&Value> = libs
            .iter()
            .filter(|l| l["name"].as_str().unwrap().starts_with("org.lwjgl:lwjgl:"))
            .collect();
        assert_eq!(lwjgl.len(), 1);
        assert_eq!(lwjgl[0]["name"], "org.lwjgl:lwjgl:3.3.2");
        // 父独有的 brigadier 保留
        assert!(libs.iter().any(|l| l["name"] == "com.mojang:brigadier:1.2.9"));
    }

    #[test]
    fn merge_arguments_preserves_child_repeated_flags() {
        // Forge 官方 jvm 数组的重复 flag（--add-opens x2）必须原样保留
        let parent = json!({ "arguments": { "jvm": ["-cp", "${classpath}"] } });
        let child = json!({ "arguments": { "jvm": [
            "--add-opens", "java.base/java.util.jar=cpw.mods.securejarhandler",
            "--add-opens", "java.base/java.lang.invoke=cpw.mods.securejarhandler",
            "--add-exports", "java.base/sun.security.util=cpw.mods.securejarhandler",
            "--add-exports", "jdk.naming.dns/com.sun.jndi.dns=java.naming"
        ]}});
        let merged = merge_arguments(parent.get("arguments"), child.get("arguments")).unwrap();
        let jvm = merged["jvm"].as_array().unwrap();
        let toks: Vec<&str> = jvm.iter().map(|v| v.as_str().unwrap()).collect();
        // 子内部重复 flag 保留；与父重复的 "-cp" 不再追加
        assert_eq!(
            toks,
            vec![
                "-cp",
                "${classpath}",
                "--add-opens",
                "java.base/java.util.jar=cpw.mods.securejarhandler",
                "--add-opens",
                "java.base/java.lang.invoke=cpw.mods.securejarhandler",
                "--add-exports",
                "java.base/sun.security.util=cpw.mods.securejarhandler",
                "--add-exports",
                "jdk.naming.dns/com.sun.jndi.dns=java.naming"
            ]
        );
    }

    #[test]
    fn inherits_cycle_detected() {
        let fixtures = HashMap::from([
            ("a".to_string(), json!({ "id": "a", "inheritsFrom": "b" })),
            ("b".to_string(), json!({ "id": "b", "inheritsFrom": "a" })),
        ]);
        let fetcher = |id: &str| fixtures.get(id).cloned().ok_or_else(|| AppError::msg("缺失"));
        let err = resolve_with("a", &fetcher).unwrap_err();
        assert!(err.to_string().contains("循环"), "应报循环错误，实际：{err}");
    }
}
