// 更新检查：GitHub Releases 最新正式版探测 + 版本比较
//
// 数据源：GET https://api.github.com/repos/R3-141/PieMCL/releases/latest
// （无需鉴权；端点自动排除 draft 与 prerelease）。解析 tag_name / body（更新
// 说明）/ html_url（发布页）。tag 形如 "v26.10.27"，剥离 v 前缀后与当前版本
// （Cargo.toml，CalVer）逐段数值比较。
//
// 触发：启动后静默检查（失败静默）+ 设置页手动检查（失败给出原因）。
// 用户控制：update_check_enabled（启动检查开关）/ update_skipped_version
// （跳过的版本），存于 settings.json。

use serde::Serialize;
use serde_json::Value;

use super::error::{AppError, AppResult};

const RELEASES_LATEST_API: &str =
    "https://api.github.com/repos/R3-141/PieMCL/releases/latest";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 更新检查结果（IPC 返回）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    /// 是否有新版本
    pub has_update: bool,
    /// 当前版本
    pub current_version: String,
    /// 最新正式版版本号（已剥离 v 前缀）
    pub latest_version: String,
    /// 更新说明（Release body，Markdown 源码）
    pub notes: String,
    /// 发布页链接（GitHub Releases）
    pub html_url: String,
}

fn client() -> AppResult<reqwest::Client> {
    crate::core::net::apply_proxy(reqwest::Client::builder()
        .user_agent(concat!("PieMCL/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TOTAL_TIMEOUT))
        .build()
        .map_err(|e| AppError::msg(format!("构建 HTTP 客户端失败：{e}")))
}

/// 版本号解析为数值段：剥离前缀 v/V 与构建元数据，"26.10.27" → [26, 10, 27]。
/// 年份归一化：首段 ≥2000 时减 2000（GitHub tag 用 v2026.10.26 四位年份，
/// 应用版本是 26.10.26 两位年份，比较前必须对齐，否则永远误报有更新）。
/// 非 JSON：数字段不足时以 0 补齐
fn parse_version(v: &str) -> Vec<u64> {
    let core = v.trim().trim_start_matches(|c| c == 'v' || c == 'V');
    let core = core.split(['-', '+']).next().unwrap_or(core);
    let mut segs: Vec<u64> = core
        .split('.')
        .map(|seg| seg.trim().parse::<u64>().unwrap_or(0))
        .collect();
    if let Some(first) = segs.first_mut() {
        if *first >= 2000 {
            *first -= 2000;
        }
    }
    segs
}

/// 版本比较：a > b 返回 true（逐段数值比较，短版本以 0 补齐）
pub fn is_newer_version(a: &str, b: &str) -> bool {
    let (pa, pb) = (parse_version(a), parse_version(b));
    let len = pa.len().max(pb.len());
    for i in 0..len {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 拉取 GitHub 最新 Release 并与当前版本比较
pub async fn check_update() -> AppResult<UpdateCheckResult> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let resp = client()?
        .get(RELEASES_LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::msg(format!("连接 GitHub 失败：{e}（可尝试在设置中配置代理）")))?;
    if !resp.status().is_success() {
        return Err(AppError::msg(format!(
            "GitHub 返回异常状态：{}",
            resp.status()
        )));
    }
    let value: Value = resp
        .json()
        .await
        .map_err(|e| AppError::msg(format!("解析 GitHub 响应失败：{e}")))?;
    let latest = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::msg("GitHub 响应缺少 tag_name"))?
        .trim()
        .trim_start_matches('v')
        .to_string();
    let notes = value
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let html_url = value
        .get("html_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("https://github.com/R3-141/PieMCL/releases")
        .to_string();

    let has_update = is_newer_version(&latest, &current);
    Ok(UpdateCheckResult {
        has_update,
        current_version: current,
        latest_version: latest,
        notes,
        html_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_rules() {
        assert_eq!(parse_version("26.10.27"), vec![26, 10, 27]);
        assert_eq!(parse_version("v26.10.27"), vec![26, 10, 27]);
        assert_eq!(parse_version("V1.2"), vec![1, 2]);
        assert_eq!(parse_version("1.2.3-beta.1"), vec![1, 2, 3]);
        assert_eq!(parse_version("1.2.3+build.5"), vec![1, 2, 3]);
        assert_eq!(parse_version(""), vec![0]);
        // 年份归一化：GitHub tag 四位年份 → 应用两位年份
        assert_eq!(parse_version("v2026.10.26"), vec![26, 10, 26]);
    }

    #[test]
    fn newer_version_rules() {
        // 更新
        assert!(is_newer_version("26.10.27", "26.10.26"));
        assert!(is_newer_version("27.0.0", "26.10.26"));
        assert!(is_newer_version("26.11.0", "26.10.99"));
        assert!(is_newer_version("v26.10.27", "26.10.26"));
        // 段数不同：短版本以 0 补齐
        assert!(is_newer_version("26.10.26.1", "26.10.26"));
        assert!(is_newer_version("26.10.27", "26.10"));
        // 相同 / 更旧
        assert!(!is_newer_version("26.10.26", "26.10.26"));
        assert!(!is_newer_version("26.10.25", "26.10.26"));
        assert!(!is_newer_version("26.9.9", "26.10.0"));
        // 四位年份 tag 与两位年份版本号等价比较（不误报）
        assert!(!is_newer_version("v2026.10.26", "26.10.26"));
        assert!(is_newer_version("v2026.10.27", "26.10.26"));
    }
}
