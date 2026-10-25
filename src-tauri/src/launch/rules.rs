// 启动参数条件规则评估
//
// 语义与 Mojang 库下载规则一致：无规则默认允许；有规则默认排除，
// 按序应用所有匹配规则的 action（后匹配者覆盖）。
// 启动参数场景的 features 由启动器提供：
// - has_custom_resolution：实例自定义了窗口尺寸或全屏
// - is_demo_user / has_quick_plays_support 等：本启动器恒为 false / 不启用

use serde_json::Value;

/// 启动时启用的 feature 集合
#[derive(Debug, Clone, Default)]
pub struct LaunchFeatures {
    pub has_custom_resolution: bool,
}

impl LaunchFeatures {
    fn contains(&self, name: &str) -> bool {
        match name {
            "has_custom_resolution" => self.has_custom_resolution,
            // 其余 feature（is_demo_user 等）本启动器一律未启用
            _ => false,
        }
    }
}

/// 评估规则是否允许；rules 为 version.json 中的 rules 数组（可为 None）
pub fn rules_allow(rules: Option<&Value>, features: &LaunchFeatures) -> bool {
    let Some(arr) = rules.and_then(|r| r.as_array()) else {
        return true;
    };
    let mut allow = false;
    for rule in arr {
        // features 规则：声明的所有 feature 都必须启用才算匹配
        if let Some(f) = rule.get("features").and_then(|f| f.as_object()) {
            let all_enabled = f
                .keys()
                .all(|k| features.contains(k) == f.get(k).and_then(|v| v.as_bool()).unwrap_or(false));
            if !all_enabled {
                continue;
            }
        }
        // os 规则：name 与 arch 都需匹配（缺省 = 任意）
        if let Some(os) = rule.get("os") {
            if let Some(name) = os.get("name").and_then(|n| n.as_str()) {
                if name != "windows" {
                    continue;
                }
            }
            if let Some(arch) = os.get("arch").and_then(|a| a.as_str()) {
                if arch != "x86" {
                    // Mojang 的 os.arch 取自 System.getProperty("os.arch")，Windows x64 = "x86"
                    continue;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_rules_allows() {
        assert!(rules_allow(None, &LaunchFeatures::default()));
    }

    #[test]
    fn os_rules() {
        let f = LaunchFeatures::default();
        let osx_only = json!([{ "action": "allow", "os": { "name": "osx" } }]);
        assert!(!rules_allow(Some(&osx_only), &f));
        let windows_only = json!([{ "action": "allow", "os": { "name": "windows" } }]);
        assert!(rules_allow(Some(&windows_only), &f));
        // os.arch：Windows x64 的 os.arch 属性为 "x86"
        let arch = json!([{ "action": "allow", "os": { "name": "windows", "arch": "x86" } }]);
        assert!(rules_allow(Some(&arch), &f));
        let bad_arch = json!([{ "action": "allow", "os": { "name": "windows", "arch": "arm" } }]);
        assert!(!rules_allow(Some(&bad_arch), &f));
    }

    #[test]
    fn feature_rules() {
        let mut f = LaunchFeatures::default();
        let res_rule = json!([{ "action": "allow", "features": { "has_custom_resolution": true } }]);
        assert!(!rules_allow(Some(&res_rule), &f), "未启用 feature → 排除");
        f.has_custom_resolution = true;
        assert!(rules_allow(Some(&res_rule), &f), "启用 feature → 允许");
        // demo 规则（本启动器恒 false）
        let demo = json!([{ "action": "allow", "features": { "is_demo_user": true } }]);
        assert!(!rules_allow(Some(&demo), &f));
    }

    #[test]
    fn last_match_wins() {
        let f = LaunchFeatures::default();
        let paired = json!([
            { "action": "allow", "os": { "name": "windows" } },
            { "action": "disallow", "features": { "is_demo_user": true } }
        ]);
        assert!(rules_allow(Some(&paired), &f));
        let inverted = json!([
            { "action": "disallow", "features": { "is_demo_user": true } },
            { "action": "allow", "os": { "name": "windows" } }
        ]);
        assert!(rules_allow(Some(&inverted), &f));
    }
}
