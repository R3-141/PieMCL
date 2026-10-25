// 游戏配置：启动器全局配置模板 + 批量应用到实例
//
// 数据流（PCL 式全局设置）：
//   模板（{启动器目录}/game-config.json，用户在页面直接编辑并「保存」）
//     → 「应用到实例」弹窗多选 → 逐实例 write_game_config 原位改写 options.txt
//
// 模板语义：字段缺失（None）= 未设置，应用时跳过 —— 只同步用户明确调过的项。
// 版本差异在写入层吸收：
// - 视场角 fov：≤1.7.10 存绝对角度（70 = Normal）；1.8+ 存 -1~1
//   （度数 = 70 + 40 × 值，0 = Normal、1 = Quake Pro 110°）
// - 主音量：soundCategory_master / music 两代键名，按文件中已存在者写入
// - 键位：1.13+ 存符号名（key.keyboard.w）；≤1.12.2 存 LWJGL2 键码
//   （W=17，鼠标左键=-100）——收录常用约 27 键的双向映射
// - autoJump：<1.10 无此键，自动跳过
//
// 安全原则：只改写认识的标准键（原位替换或追加），其余行零接触、不重排、
// 不删除；lang 键永不触碰（与启动器的中文默认逻辑互不干扰）。

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;

/// 模板文件名（位于启动器私有目录）
const TEMPLATE_FILE: &str = "game-config.json";

/// 已收录的常用键位：action id（options 行为 `key_{action}`）+ 默认符号
///（绑定交互为「点击后按键」，绑定值可为下方符号表内任意键，不必是默认键）
struct KeySpec {
    action: &'static str,
    symbol: &'static str,
}

const KEY_SPECS: &[KeySpec] = &[
    KeySpec { action: "key.forward", symbol: "key.keyboard.w" },
    KeySpec { action: "key.left", symbol: "key.keyboard.a" },
    KeySpec { action: "key.back", symbol: "key.keyboard.s" },
    KeySpec { action: "key.right", symbol: "key.keyboard.d" },
    KeySpec { action: "key.jump", symbol: "key.keyboard.space" },
    KeySpec { action: "key.sneak", symbol: "key.keyboard.left.shift" },
    KeySpec { action: "key.sprint", symbol: "key.keyboard.left.control" },
    KeySpec { action: "key.inventory", symbol: "key.keyboard.e" },
    KeySpec { action: "key.drop", symbol: "key.keyboard.q" },
    KeySpec { action: "key.chat", symbol: "key.keyboard.t" },
    KeySpec { action: "key.command", symbol: "key.keyboard.slash" },
    KeySpec { action: "key.swapOffhand", symbol: "key.keyboard.f" },
    KeySpec { action: "key.playerlist", symbol: "key.keyboard.tab" },
    KeySpec { action: "key.fullscreen", symbol: "key.keyboard.f11" },
    KeySpec { action: "key.screenshot", symbol: "key.keyboard.f2" },
    KeySpec { action: "key.togglePerspective", symbol: "key.keyboard.f5" },
    KeySpec { action: "key.toggleDebugF3", symbol: "key.keyboard.f3" },
    KeySpec { action: "key.attack", symbol: "key.mouse.left" },
    KeySpec { action: "key.use", symbol: "key.mouse.right" },
    KeySpec { action: "key.pickItem", symbol: "key.mouse.middle" },
    KeySpec { action: "key.hotbar.1", symbol: "key.keyboard.1" },
    KeySpec { action: "key.hotbar.2", symbol: "key.keyboard.2" },
    KeySpec { action: "key.hotbar.3", symbol: "key.keyboard.3" },
    KeySpec { action: "key.hotbar.4", symbol: "key.keyboard.4" },
    KeySpec { action: "key.hotbar.5", symbol: "key.keyboard.5" },
    KeySpec { action: "key.hotbar.6", symbol: "key.keyboard.6" },
    KeySpec { action: "key.hotbar.7", symbol: "key.keyboard.7" },
    KeySpec { action: "key.hotbar.8", symbol: "key.keyboard.8" },
    KeySpec { action: "key.hotbar.9", symbol: "key.keyboard.9" },
];

/// 合法键位符号 → ≤1.12.2 的 LWJGL2 键码（鼠标键为负数）。
/// 绑定值为此表内的符号即可跨版本写入。
const SYMBOL_LEGACY: &[(&str, &str)] = &[
    // 字母（LWJGL2 非连续布局）
    ("key.keyboard.a", "30"),
    ("key.keyboard.b", "48"),
    ("key.keyboard.c", "46"),
    ("key.keyboard.d", "32"),
    ("key.keyboard.e", "18"),
    ("key.keyboard.f", "33"),
    ("key.keyboard.g", "34"),
    ("key.keyboard.h", "35"),
    ("key.keyboard.i", "23"),
    ("key.keyboard.j", "36"),
    ("key.keyboard.k", "37"),
    ("key.keyboard.l", "38"),
    ("key.keyboard.m", "50"),
    ("key.keyboard.n", "49"),
    ("key.keyboard.o", "24"),
    ("key.keyboard.p", "25"),
    ("key.keyboard.q", "16"),
    ("key.keyboard.r", "19"),
    ("key.keyboard.s", "31"),
    ("key.keyboard.t", "20"),
    ("key.keyboard.u", "22"),
    ("key.keyboard.v", "47"),
    ("key.keyboard.w", "17"),
    ("key.keyboard.x", "45"),
    ("key.keyboard.y", "21"),
    ("key.keyboard.z", "44"),
    // 数字行（1 从 2 起，0 在末位）
    ("key.keyboard.1", "2"),
    ("key.keyboard.2", "3"),
    ("key.keyboard.3", "4"),
    ("key.keyboard.4", "5"),
    ("key.keyboard.5", "6"),
    ("key.keyboard.6", "7"),
    ("key.keyboard.7", "8"),
    ("key.keyboard.8", "9"),
    ("key.keyboard.9", "10"),
    ("key.keyboard.0", "11"),
    // 功能键
    ("key.keyboard.f1", "59"),
    ("key.keyboard.f2", "60"),
    ("key.keyboard.f3", "61"),
    ("key.keyboard.f4", "62"),
    ("key.keyboard.f5", "63"),
    ("key.keyboard.f6", "64"),
    ("key.keyboard.f7", "65"),
    ("key.keyboard.f8", "66"),
    ("key.keyboard.f9", "67"),
    ("key.keyboard.f10", "68"),
    ("key.keyboard.f11", "87"),
    ("key.keyboard.f12", "88"),
    // 控制键
    ("key.keyboard.escape", "1"),
    ("key.keyboard.tab", "15"),
    ("key.keyboard.enter", "28"),
    ("key.keyboard.backspace", "14"),
    ("key.keyboard.space", "57"),
    ("key.keyboard.caps.lock", "58"),
    ("key.keyboard.left.shift", "42"),
    ("key.keyboard.right.shift", "54"),
    ("key.keyboard.left.control", "29"),
    ("key.keyboard.right.control", "157"),
    ("key.keyboard.left.alt", "56"),
    ("key.keyboard.right.alt", "184"),
    // 方向与编辑区
    ("key.keyboard.up", "200"),
    ("key.keyboard.down", "208"),
    ("key.keyboard.left", "203"),
    ("key.keyboard.right", "205"),
    ("key.keyboard.insert", "210"),
    ("key.keyboard.delete", "211"),
    ("key.keyboard.home", "199"),
    ("key.keyboard.end", "207"),
    ("key.keyboard.page.up", "201"),
    ("key.keyboard.page.down", "209"),
    // 标点
    ("key.keyboard.minus", "12"),
    ("key.keyboard.equal", "13"),
    ("key.keyboard.left.bracket", "26"),
    ("key.keyboard.right.bracket", "27"),
    ("key.keyboard.backslash", "43"),
    ("key.keyboard.semicolon", "39"),
    ("key.keyboard.apostrophe", "40"),
    ("key.keyboard.grave", "41"),
    ("key.keyboard.comma", "51"),
    ("key.keyboard.period", "52"),
    ("key.keyboard.slash", "53"),
    // 鼠标
    ("key.mouse.left", "-100"),
    ("key.mouse.right", "-99"),
    ("key.mouse.middle", "-98"),
];

/// 符号是否为可绑定的合法键位
fn is_valid_symbol(symbol: &str) -> bool {
    SYMBOL_LEGACY.iter().any(|(s, _)| *s == symbol)
}

/// 符号 → ≤1.12.2 的 LWJGL2 键码；不在表内返回 None（写入时跳过该键）
fn symbol_to_legacy(symbol: &str) -> Option<&'static str> {
    SYMBOL_LEGACY
        .iter()
        .find(|(s, _)| *s == symbol)
        .map(|(_, c)| *c)
}

/// 游戏配置模板（启动器全局持久化 + 应用到实例的写入补丁同构）。
/// 字段缺失 = 未设置，应用时跳过 —— 只同步用户明确设置的项。
/// 序列化跳过 None（null 会被前端误判为"已设置为 null"）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameConfigTemplate {
    /// 视场角（度数 30–110，UI 层值；写入时按版本换算存储格式）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fov: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gui_scale: Option<f64>,
    /// 亮度（0–1）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fullscreen: Option<bool>,
    /// 鼠标灵敏度（0–1，0.5 = 游戏默认）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mouse_sensitivity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_jump: Option<bool>,
    /// 主音量（0–1）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_volume: Option<f64>,
    /// 键位：action id（如 "key.forward"）→ 1.13+ 符号名（仅收录表内条目生效）
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub keys: HashMap<String, String>,
}

/// 解析 MC 版本首两段；非 "1.x" 形态（rd-/a1./b1.）返回 (1, 0)
fn version_parts(mc_version: &str) -> (u32, u32) {
    let mut parts = mc_version.trim().split('.');
    let first = parts.next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(1);
    let second = parts.next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    (first, second)
}

/// 是否早于 1.8（fov 存绝对角度）
fn is_pre_1_8(mc_version: &str) -> bool {
    let (a, b) = version_parts(mc_version);
    a == 1 && b < 8
}

/// 是否早于 1.10（无 autoJump 键）
fn is_pre_1_10(mc_version: &str) -> bool {
    let (a, b) = version_parts(mc_version);
    a == 1 && b < 10
}

/// 是否早于 1.13（键位存 LWJGL2 键码）
fn is_pre_1_13(mc_version: &str) -> bool {
    let (a, b) = version_parts(mc_version);
    a == 1 && b < 13
}

/// 视场角：度数 → options.txt 存储值
fn fov_to_storage(degrees: f64, pre18: bool) -> f64 {
    if pre18 {
        degrees
    } else {
        (degrees - 70.0) / 40.0
    }
}

/// 视场角：options.txt 存储值 → 度数
#[cfg(test)]
fn fov_from_storage(v: f64, pre18: bool) -> f64 {
    if pre18 {
        v
    } else {
        70.0 + 40.0 * v
    }
}

/// 原位改写一个键：已存在则替换该行，否则追加到末尾
fn set_key(lines: &mut Vec<String>, key: &str, value: &str) {
    let prefix = format!("{key}:");
    if let Some(line) = lines.iter_mut().find(|l| l.starts_with(&prefix)) {
        *line = format!("{key}:{value}");
    } else {
        lines.push(format!("{key}:{value}"));
    }
}

fn get_value_owned(lines: &[String], key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    lines
        .iter()
        .find(|l| l.starts_with(&prefix))
        .map(|l| l[prefix.len()..].trim().to_string())
}

// ---------------------------------------------------------------------------
// 模板持久化
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn load_game_config(
    state: State<'_, AppState>,
) -> Result<GameConfigTemplate, String> {
    let path = state.paths.root().join(TEMPLATE_FILE);
    match tokio::fs::read_to_string(&path).await {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("game-config.json 解析失败：{e}")),
        Err(_) => Ok(GameConfigTemplate::default()),
    }
}

#[tauri::command]
pub async fn save_game_config(
    state: State<'_, AppState>,
    template: GameConfigTemplate,
) -> Result<(), String> {
    // 取值范围校验
    if let Some(deg) = template.fov {
        if !(30.0..=110.0).contains(&deg) {
            return Err("视场角需在 30–110 之间".into());
        }
    }
    if let Some(v) = template.mouse_sensitivity {
        if !(0.0..=1.0).contains(&v) {
            return Err("鼠标灵敏度需在 0–1 之间".into());
        }
    }
    if let Some(v) = template.brightness {
        if !(0.0..=1.0).contains(&v) {
            return Err("亮度需在 0–1 之间".into());
        }
    }
    if let Some(v) = template.master_volume {
        if !(0.0..=1.0).contains(&v) {
            return Err("主音量需在 0–1 之间".into());
        }
    }
    // 键位仅接受符号表内的合法键位（绑定值不必是该 action 的默认键）
    for (action, symbol) in &template.keys {
        if KEY_SPECS.iter().all(|s| s.action != action) {
            return Err(format!("未收录的键位：{action}"));
        }
        if !is_valid_symbol(symbol) {
            return Err(format!("键位 {action} 的取值无效：{symbol}"));
        }
    }

    let path = state.paths.root().join(TEMPLATE_FILE);
    let text = serde_json::to_string_pretty(&template)
        .map_err(|e| format!("序列化配置失败：{e}"))?;
    tokio::fs::write(&path, text)
        .await
        .map_err(|e| format!("保存配置失败：{e}"))
}

// ---------------------------------------------------------------------------
// 应用到实例
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn write_game_config(
    state: State<'_, AppState>,
    instance_id: String,
    template: GameConfigTemplate,
) -> Result<(), String> {
    let detail = state.instances.get(&instance_id).await.map_err(String::from)?;
    let mc_version = detail.summary.mc_version.clone();
    let game_dir = state
        .instances
        .game_dir(&instance_id)
        .await
        .map_err(String::from)?;
    let path: PathBuf = game_dir.join("options.txt");

    let mut lines: Vec<String> = {
        let text = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        // 保留非「k:v」行（注释等），只按序改写/追加键值行
        text.lines().map(String::from).collect()
    };

    let pre18 = is_pre_1_8(&mc_version);
    if let Some(deg) = template.fov {
        let v = fov_to_storage(deg, pre18);
        set_key(&mut lines, "fov", &format!("{v:.3}"));
    }
    if let Some(v) = template.gui_scale {
        set_key(&mut lines, "guiScale", &format!("{}", v as i32));
    }
    if let Some(v) = template.brightness {
        set_key(&mut lines, "brightness", &format!("{v:.3}"));
    }
    if let Some(v) = template.render_distance {
        set_key(&mut lines, "renderDistance", &format!("{}", v as i32));
    }
    if let Some(v) = template.max_fps {
        set_key(&mut lines, "maxFps", &format!("{}", v as i32));
    }
    if let Some(v) = template.fullscreen {
        set_key(&mut lines, "fullscreen", if v { "true" } else { "false" });
    }
    if let Some(v) = template.mouse_sensitivity {
        set_key(&mut lines, "mouseSensitivity", &format!("{v:.3}"));
    }
    // autoJump 为 1.10+ 键，老版本写入无意义，跳过
    if let Some(v) = template.auto_jump {
        if !is_pre_1_10(&mc_version) {
            set_key(&mut lines, "autoJump", if v { "true" } else { "false" });
        }
    }
    if let Some(v) = template.master_volume {
        // 两代键名：优先写文件中已存在者，都不存在时写现代键
        let key = if get_value_owned(&lines, "soundCategory_master").is_some() {
            "soundCategory_master"
        } else if get_value_owned(&lines, "music").is_some() {
            "music"
        } else {
            "soundCategory_master"
        };
        set_key(&mut lines, key, &format!("{v:.3}"));
    }
    // 键位：action 必须在收录表内，符号必须在合法键位表内；老版本转换为 LWJGL2 键码
    //（表外符号对老版本无法转换，跳过并继续，避免阻塞整体应用）
    let pre113 = is_pre_1_13(&mc_version);
    for (action, symbol) in &template.keys {
        if KEY_SPECS.iter().all(|s| s.action != action) {
            continue;
        }
        let stored = if pre113 {
            let Some(code) = symbol_to_legacy(symbol) else {
                continue;
            };
            code
        } else if is_valid_symbol(symbol) {
            symbol.as_str()
        } else {
            continue;
        };
        set_key(&mut lines, &format!("key_{action}"), stored);
    }

    // lang 键永不触碰（不在上方改写集合内，天然满足）
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建游戏目录失败：{e}"))?;
    }
    tokio::fs::write(&path, out)
        .await
        .map_err(|e| format!("写入 options.txt 失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fov_conversion_matrix() {
        // 1.8+：度数 = 70 + 40 × 值
        assert_eq!(fov_to_storage(70.0, false), 0.0);
        assert_eq!(fov_to_storage(110.0, false), 1.0);
        assert_eq!(fov_to_storage(30.0, false), -1.0);
        assert_eq!(fov_from_storage(0.0, false), 70.0);
        assert_eq!(fov_from_storage(1.0, false), 110.0);
        // ≤1.7.10：绝对角度
        assert_eq!(fov_to_storage(70.0, true), 70.0);
        assert_eq!(fov_from_storage(90.0, true), 90.0);
    }

    #[test]
    fn version_branches() {
        assert!(is_pre_1_8("1.7.10"));
        assert!(is_pre_1_8("1.8") == false);
        assert!(is_pre_1_8("1.12.2") == false);
        assert!(is_pre_1_8("1.8.9") == false);
        assert!(is_pre_1_8("a1.2.5")); // 经典版按 pre-1.8 处理
        assert!(is_pre_1_8("26.2") == false);
        assert!(is_pre_1_10("1.8.9"));
        assert!(is_pre_1_10("1.10") == false);
        assert!(is_pre_1_13("1.12.2"));
        assert!(is_pre_1_13("1.13") == false);
    }

    #[test]
    fn legacy_key_codes_unique() {
        // 符号与键码一一对应（无重复值）；收录键位的默认符号全部可转换
        for (i, (sym, code)) in SYMBOL_LEGACY.iter().enumerate() {
            assert_eq!(symbol_to_legacy(sym), Some(*code));
            for (other_sym, other_code) in &SYMBOL_LEGACY[i + 1..] {
                assert_ne!(code, other_code, "键码重复：{sym}");
                assert_ne!(sym, other_sym, "符号重复：{sym}");
            }
        }
        for spec in KEY_SPECS {
            assert!(
                is_valid_symbol(spec.symbol),
                "收录键位默认符号应合法：{}",
                spec.action
            );
        }
        assert!(symbol_to_legacy("key.keyboard.unknown").is_none());
    }

    #[test]
    fn set_key_replaces_in_place_and_appends() {
        let mut lines = vec!["fov:0.0".into(), "lang:zh_cn".into()];
        set_key(&mut lines, "fov", "0.25");
        assert_eq!(lines[0], "fov:0.25");
        set_key(&mut lines, "mouseSensitivity", "0.5");
        assert_eq!(lines[2], "mouseSensitivity:0.5");
        assert_eq!(lines[1], "lang:zh_cn", "无关行保持原位");
    }

    #[test]
    fn template_json_roundtrip() {
        let mut t = GameConfigTemplate::default();
        t.fov = Some(90.0);
        t.keys.insert("key.forward".into(), "key.keyboard.w".into());
        let text = serde_json::to_string(&t).unwrap();
        let back: GameConfigTemplate = serde_json::from_str(&text).unwrap();
        assert_eq!(back.fov, Some(90.0));
        assert_eq!(back.keys.get("key.forward").map(String::as_str), Some("key.keyboard.w"));
        // 空模板序列化不产生 null 字段（null 会被前端误判为"已设置"）
        let empty = serde_json::to_string(&GameConfigTemplate::default()).unwrap();
        assert_eq!(empty, "{}");
        let back2: GameConfigTemplate = serde_json::from_str(&empty).unwrap();
        assert!(back2.fov.is_none());
        assert!(back2.keys.is_empty());
    }
}
