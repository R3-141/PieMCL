// 模组管理模块（阶段五）
//
// - parse.rs：jar 元数据解析（fabric.mod.json / mods.toml / MANIFEST.MF，逐级降级）
// - local.rs：本地管理（扫描 / 启停重命名 / 回收站 / 拖放复制 / 缺失依赖计算）
// - store.rs：安装来源记录 {实例根}/mods.json（更新检查与整合包导出的数据基础）
// - modrinth.rs：Modrinth API v2 封装（搜索 / 版本 / 依赖解析）
// - curseforge.rs：CurseForge API v1 封装（需用户 API Key）
// - pack.rs：整合包解析（mrpack / CF zip）与 .mrpack 导出

pub mod curseforge;
pub mod local;
pub mod modrinth;
pub mod pack;
pub mod parse;
pub mod store;

/// 社区资源类型：模组 / 数据包 / 光影包 / 资源包。
/// 决定落位目录、Modrinth project_type、本地文件扩展名与能力差异
/// （启停 / 依赖 / CurseForge / .mrpack 导出仅模组支持）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Mod,
    Datapack,
    Shader,
    ResourcePack,
}

impl ResourceType {
    /// 命令参数解析（前端传 resource_type 字符串）
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "mod" => Ok(Self::Mod),
            "datapack" => Ok(Self::Datapack),
            "shader" => Ok(Self::Shader),
            "resource_pack" => Ok(Self::ResourcePack),
            _ => Err(format!("未知的资源类型：{s}")),
        }
    }

    /// 实例 minecraft/ 下的落位目录名
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::Datapack => "datapacks",
            Self::Shader => "shaderpacks",
            Self::ResourcePack => "resourcepacks",
        }
    }

    /// 来源记录 key 前缀（mods.json key = "{prefix}:{基名}"）
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::Datapack => "datapack",
            Self::Shader => "shader",
            Self::ResourcePack => "resource_pack",
        }
    }

    /// 本地安装接受的文件扩展名（模组 .jar，其余 .zip）
    pub fn file_ext(self) -> &'static str {
        match self {
            Self::Mod => ".jar",
            _ => ".zip",
        }
    }

    /// Modrinth project_type（搜索 facet 与兼容过滤用）。
    /// 注意：搜索 facet 的资源包取值为 "resourcepack"（无下划线，与 API 实测一致）
    pub fn modrinth_project_type(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Datapack => "datapack",
            Self::Shader => "shader",
            Self::ResourcePack => "resourcepack",
        }
    }

    /// 中文名（提示文案用）
    pub fn label(self) -> &'static str {
        match self {
            Self::Mod => "模组",
            Self::Datapack => "数据包",
            Self::Shader => "光影包",
            Self::ResourcePack => "资源包",
        }
    }

    /// 是否为模组（启停 / 依赖树 / CurseForge / .mrpack 导出仅模组支持）
    pub fn is_mod(self) -> bool {
        self == Self::Mod
    }
}
