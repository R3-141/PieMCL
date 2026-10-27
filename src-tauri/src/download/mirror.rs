// 下载镜像源改写
//
// 采用 BMCLAPI 路径方案（社区事实标准）按官方域名映射：
//   libraries.minecraft.net/x         → {镜像}/maven/x
//   piston-meta.mojang.com/x          → {镜像}/piston-meta/x
//   launchermeta.mojang.com/x         → {镜像}/piston-meta/x
//   piston-data.mojang.com/x          → {镜像}/piston-data/x
//   launcher.mojang.com/x             → {镜像}/piston-data/x
//   resources.download.minecraft.net  → {镜像}/assets
// 非官方域名（Fabric/Quilt maven 等）不改写，直接走官方源。

/// 官方 URL 前缀 → 镜像路径前缀
const MIRROR_RULES: &[(&str, &str)] = &[
    ("https://libraries.minecraft.net/", "maven/"),
    ("https://piston-meta.mojang.com/", "piston-meta/"),
    ("https://launchermeta.mojang.com/", "piston-meta/"),
    ("https://piston-data.mojang.com/", "piston-data/"),
    ("https://launcher.mojang.com/", "piston-data/"),
    ("https://resources.download.minecraft.net/", "assets/"),
];

/// 把 Mojang 官方 URL 改写为镜像 URL；非官方域名返回 None
pub fn rewrite_with_mirror(mirror_base: &str, official_url: &str) -> Option<String> {
    let base = mirror_base.trim_end_matches('/');
    for (prefix, mapped) in MIRROR_RULES {
        if let Some(rest) = official_url.strip_prefix(prefix) {
            return Some(format!("{base}/{mapped}{rest}"));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// MCIMirror（mod.mcimirror.top）Modrinth / CurseForge 镜像
//
// 按 MCIMirror 官方替换表（https://www.mcimirror.top/guide/start/getting-started）：
//   https://cdn.modrinth.com/x   → https://mod.mcimirror.top/x
//   https://edge.forgecdn.net/x  → https://mod.mcimirror.top/x
// 注意：mediafilez.forgecdn.net 为 CurseForge 私有源，镜像不覆盖，不做替换。
// 路径与参数同上游一致，仅换域名。

const MCIMIRROR_BASE: &str = "https://mod.mcimirror.top";

/// 可被 MCIMirror 镜像的官方 URL 前缀
const MCIMIRROR_RULES: &[&str] = &[
    "https://cdn.modrinth.com/",
    "https://edge.forgecdn.net/",
];

/// 把 Modrinth / CurseForge CDN URL 改写为 MCIMirror URL；不可镜像的域名返回 None
pub fn rewrite_with_mcimirror(official_url: &str) -> Option<String> {
    for prefix in MCIMIRROR_RULES {
        if let Some(rest) = official_url.strip_prefix(prefix) {
            if rest.is_empty() {
                return None;
            }
            return Some(format!("{MCIMIRROR_BASE}/{rest}"));
        }
    }
    None
}

/// 生成 Modrinth / CurseForge 文件下载候选（镜像优先），去重。
/// 不可镜像的 URL（mediafilez.forgecdn.net 等）原样保留。
pub fn mirror_first_candidates(urls: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(urls.len() * 2);
    for u in urls {
        if let Some(m) = rewrite_with_mcimirror(u) {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    for u in urls {
        if !out.contains(u) {
            out.push(u.clone());
        }
    }
    out
}

/// 官方源优先 + MCIMirror 兜底的候选（纯函数：数量多、对延迟敏感的小资源）
pub fn official_first_candidates(urls: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(urls.len() * 2);
    for u in urls {
        if !out.contains(u) {
            out.push(u.clone());
        }
    }
    for u in urls {
        if let Some(m) = rewrite_with_mcimirror(u) {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

/// 按「模组市场 · 下载源」设置生成下载候选：所选源在前、另一源始终兜底。
/// 模组 / 整合包文件与市场图标等所有 MCImirror 可镜像资源的统一入口。
pub fn market_candidates(urls: &[String]) -> Vec<String> {
    if crate::core::settings::market_download_source() == "official" {
        official_first_candidates(urls)
    } else {
        mirror_first_candidates(urls)
    }
}

/// 生成下载 URL 候选列表，去重。
/// official_first=true（自动策略）：官方在前、镜像在后（官方失败由引擎轮换到镜像）；
/// official_first=false（镜像优先策略）：镜像在前、官方兜底。
/// 非 Mojang 官方域名（如 Fabric maven）无论策略都只走官方源。
pub fn build_candidates(mirrors: &[String], official_url: &str, official_first: bool) -> Vec<String> {
    let mut official = vec![official_url.to_string()];
    let mut mirrored: Vec<String> = Vec::new();
    for m in mirrors {
        if let Some(u) = rewrite_with_mirror(m, official_url) {
            if !mirrored.contains(&u) {
                mirrored.push(u);
            }
        }
    }
    if mirrored.is_empty() {
        return official;
    }
    if official_first {
        official.extend(mirrored);
        official
    } else {
        mirrored.extend(official);
        mirrored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";

    #[test]
    fn rewrites_official_domains() {
        assert_eq!(
            rewrite_with_mirror(BMCLAPI, "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar").unwrap(),
            format!("{BMCLAPI}/maven/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar")
        );
        assert_eq!(
            rewrite_with_mirror(BMCLAPI, "https://piston-data.mojang.com/v1/objects/aaa/client.jar").unwrap(),
            format!("{BMCLAPI}/piston-data/v1/objects/aaa/client.jar")
        );
        assert_eq!(
            rewrite_with_mirror(BMCLAPI, "https://piston-meta.mojang.com/v1/packages/xx/5.json").unwrap(),
            format!("{BMCLAPI}/piston-meta/v1/packages/xx/5.json")
        );
        assert_eq!(
            rewrite_with_mirror(BMCLAPI, "https://resources.download.minecraft.net/ab/abcd").unwrap(),
            format!("{BMCLAPI}/assets/ab/abcd")
        );
        // 带尾斜杠的镜像 base 同样可用
        assert_eq!(
            rewrite_with_mirror("https://example.com/", "https://piston-meta.mojang.com/x.json").unwrap(),
            "https://example.com/piston-meta/x.json"
        );
    }

    #[test]
    fn leaves_unofficial_urls_alone() {
        assert!(rewrite_with_mirror(BMCLAPI, "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar").is_none());
        assert!(rewrite_with_mirror(BMCLAPI, "https://meta.quiltmc.org/v3/versions/loader").is_none());
    }

    #[test]
    fn mcimirror_rewrites_modrinth_and_forgecdn() {
        assert_eq!(
            rewrite_with_mcimirror("https://cdn.modrinth.com/data/AABBCC/versions/1.0/x.jar").unwrap(),
            "https://mod.mcimirror.top/data/AABBCC/versions/1.0/x.jar"
        );
        assert_eq!(
            rewrite_with_mcimirror("https://edge.forgecdn.net/files/1234/567/file.jar").unwrap(),
            "https://mod.mcimirror.top/files/1234/567/file.jar"
        );
        // 私有源与其余域名不做替换
        assert!(rewrite_with_mcimirror("https://mediafilez.forgecdn.net/files/1/2/f.jar").is_none());
        assert!(rewrite_with_mcimirror("https://maven.fabricmc.net/x.jar").is_none());
        assert!(rewrite_with_mcimirror("https://cdn.modrinth.com/").is_none());
    }

    #[test]
    fn mirror_first_candidates_mirror_first_official_fallback() {
        let urls = vec![
            "https://cdn.modrinth.com/data/A/x.jar".to_string(),
            "https://mediafilez.forgecdn.net/files/1/2/f.jar".to_string(),
        ];
        let c = mirror_first_candidates(&urls);
        assert_eq!(
            c,
            vec![
                "https://mod.mcimirror.top/data/A/x.jar".to_string(),
                "https://cdn.modrinth.com/data/A/x.jar".to_string(),
                "https://mediafilez.forgecdn.net/files/1/2/f.jar".to_string(),
            ]
        );
        // 不可镜像的 URL 原样保留
        let c = mirror_first_candidates(&["https://maven.fabricmc.net/x.jar".to_string()]);
        assert_eq!(c, vec!["https://maven.fabricmc.net/x.jar".to_string()]);
    }

    /// 按设置同步模组市场下载源（测试串行执行，避免全局镜像互相干扰）
    fn sync_download_source(source: &str) {
        let mut s = crate::core::settings::AppSettings::default();
        s.market_download_source = source.to_string();
        crate::core::settings::sync_market_sources(&s);
    }

    #[test]
    fn market_candidates_follows_download_source_setting() {
        let urls = vec!["https://cdn.modrinth.com/data/A/x.jar".to_string()];
        let mirror = "https://mod.mcimirror.top/data/A/x.jar".to_string();
        let official = "https://cdn.modrinth.com/data/A/x.jar".to_string();

        sync_download_source("mirror");
        assert_eq!(
            market_candidates(&urls),
            vec![mirror.clone(), official.clone()]
        );
        sync_download_source("official");
        assert_eq!(
            market_candidates(&urls),
            vec![official.clone(), mirror.clone()]
        );
        // 无论何种设置，两源均在候选中（兜底保证）
        assert_eq!(market_candidates(&urls).len(), 2);
        sync_download_source("mirror");
    }

    #[test]
    fn official_first_candidates_keeps_official_ahead() {
        let urls = vec!["https://cdn.modrinth.com/data/A/icon.png".to_string()];
        let c = official_first_candidates(&urls);
        assert_eq!(
            c,
            vec![
                "https://cdn.modrinth.com/data/A/icon.png".to_string(),
                "https://mod.mcimirror.top/data/A/icon.png".to_string(),
            ]
        );
        // 非 Modrinth/forgecdn 域名不产生额外候选
        let c = official_first_candidates(&["https://textures.minecraft.net/x.png".to_string()]);
        assert_eq!(c, vec!["https://textures.minecraft.net/x.png".to_string()]);
    }

    #[test]
    fn candidates_mirror_first_official_last() {
        let mirrors = vec![BMCLAPI.to_string()];
        // 镜像优先策略：镜像在前、官方兜底
        let c = build_candidates(
            &mirrors,
            "https://libraries.minecraft.net/a/b/c.jar",
            false,
        );
        assert_eq!(c.len(), 2);
        assert_eq!(c[0], format!("{BMCLAPI}/maven/a/b/c.jar"));
        assert_eq!(c[1], "https://libraries.minecraft.net/a/b/c.jar");

        // 自动策略（官方优先）：官方在前、镜像在后
        let c = build_candidates(
            &mirrors,
            "https://libraries.minecraft.net/a/b/c.jar",
            true,
        );
        assert_eq!(c[0], "https://libraries.minecraft.net/a/b/c.jar");
        assert_eq!(c[1], format!("{BMCLAPI}/maven/a/b/c.jar"));

        // 无镜像 → 仅官方
        let c = build_candidates(&[], "https://piston-meta.mojang.com/x.json", false);
        assert_eq!(c, vec!["https://piston-meta.mojang.com/x.json".to_string()]);

        // 非官方域名 → 仅官方
        let c = build_candidates(&mirrors, "https://maven.fabricmc.net/x.jar", true);
        assert_eq!(c, vec!["https://maven.fabricmc.net/x.jar".to_string()]);

        // 多镜像去重（同一镜像重复配置只出一个候选）
        let mirrors = vec![BMCLAPI.to_string(), format!("{BMCLAPI}/")];
        let c = build_candidates(&mirrors, "https://piston-data.mojang.com/y.jar", false);
        assert_eq!(c.len(), 2);
    }
}
