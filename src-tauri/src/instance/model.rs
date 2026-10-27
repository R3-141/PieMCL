// 实例元数据模型
//
// instance.json（实例根目录）：名称、图标、时间戳、Java / JVM 设置
// loader.json（实例根目录）：加载器类型与版本，骨架阶段 installed 恒为 false

use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_max_memory() -> u32 {
    4096
}

fn default_preset() -> String {
    "default".to_string()
}

/// JVM 启动参数设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JvmSettings {
    /// 最小内存 MB（0 = 不传 -Xms，由 JVM 自行决定）
    #[serde(default)]
    pub min_memory_mb: u32,
    /// 最大内存 MB（前端滑块范围 1024–32768）
    #[serde(default = "default_max_memory")]
    pub max_memory_mb: u32,
    /// 自动分配内存：开启时启动前按物理内存计算，忽略上面的手动值（默认开启）
    #[serde(default = "default_true")]
    pub auto_memory: bool,
    /// 附加 JVM 参数（原样追加在预设之后，可覆盖同名参数）
    #[serde(default)]
    pub extra_args: String,
    /// JVM 优化预设：default / low_spec / big_modpack
    #[serde(default = "default_preset")]
    pub preset: String,
    /// 游戏窗口宽度
    #[serde(default)]
    pub window_width: Option<u32>,
    /// 游戏窗口高度
    #[serde(default)]
    pub window_height: Option<u32>,
    /// 全屏启动（优先于自定义宽高）
    #[serde(default)]
    pub fullscreen: bool,
}

impl Default for JvmSettings {
    fn default() -> Self {
        Self {
            min_memory_mb: 0,
            max_memory_mb: default_max_memory(),
            auto_memory: true,
            extra_args: String::new(),
            preset: default_preset(),
            window_width: None,
            window_height: None,
            fullscreen: false,
        }
    }
}

/// JVM 优化预设的基础参数（选中后仍可被 extra_args 追加覆盖）
pub fn preset_args(preset: &str) -> &'static [&'static str] {
    match preset {
        "low_spec" => &[
            "-XX:+UseG1GC",
            "-XX:MaxGCPauseMillis=50",
            "-XX:G1NewSizePercent=20",
        ],
        "big_modpack" => &["-XX:+UseG1GC", "-XX:MaxGCPauseMillis=50"],
        _ => &[],
    }
}

/// instance.json 内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceMeta {
    /// 实例 id = 版本文件夹名（= 实例名清洗去重结果；重命名实例会随之变化）
    pub id: String,
    pub name: String,
    /// 绑定的 MC 版本（创建后固定，不随清单升级变化）
    pub mc_version: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// 创建时间（epoch 秒）
    pub created_at: i64,
    /// 最后启动时间（epoch 秒）
    #[serde(default)]
    pub last_launched_at: Option<i64>,
    /// 手动指定的 Java 路径；None = 自动选择
    #[serde(default)]
    pub java_path: Option<String>,
    /// 是否启用版本隔离：true = 游戏工作目录为 versions/{实例名}；
    /// false = 工作目录为 .minecraft 根（兼容极老模组）。切换不搬移已有数据
    #[serde(default = "default_true")]
    pub isolation: bool,
    #[serde(default)]
    pub jvm: JvmSettings,
}

/// loader.json 内容
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderInfo {
    /// vanilla / forge / fabric / neoforge / quilt
    /// 注意：OptiFine 是光影/优化模组而非独立加载器，仅能搭配 Forge，
    /// 通过 optifine_version 字段记录，不作为 loader_type
    #[serde(rename = "type")]
    pub loader_type: String,
    /// 加载器版本（vanilla 为 None）
    #[serde(default)]
    pub version: Option<String>,
    /// 搭配的 OptiFine 版本（仅 loader_type = forge 时可能有值）
    #[serde(default)]
    pub optifine_version: Option<String>,
    /// 加载器是否已安装（本阶段仅骨架，恒 false）
    #[serde(default)]
    pub installed: bool,
}

impl LoaderInfo {
    pub fn vanilla() -> Self {
        Self {
            loader_type: "vanilla".to_string(),
            version: None,
            optifine_version: None,
            installed: false,
        }
    }
}

/// 实例摘要（列表卡片用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSummary {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub mc_version: String,
    pub loader_type: String,
    pub loader_version: Option<String>,
    /// 搭配的 OptiFine 版本（仅 Forge 实例可能有）
    pub optifine_version: Option<String>,
    pub last_launched_at: Option<i64>,
}

/// 实例详情（编辑面板用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDetail {
    #[serde(flatten)]
    pub summary: InstanceSummary,
    pub created_at: i64,
    pub java_path: Option<String>,
    pub isolation: bool,
    pub jvm: JvmSettings,
}

/// update_instance_settings 的补丁
///
/// 注意：java_path / 窗口尺寸是三态字段——
/// 缺失 = 不修改；null = 恢复默认；有值 = 指定。
/// serde 会把 JSON null 与字段缺失都映射为双层 Option 的外层 None，
/// 因此这里用 Option<serde_json::Value> 显式区分。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstancePatch {
    pub name: Option<String>,
    #[serde(default)]
    pub java_path: Option<serde_json::Value>,
    pub min_memory_mb: Option<u32>,
    pub max_memory_mb: Option<u32>,
    /// 自动分配内存开关
    #[serde(default)]
    pub auto_memory: Option<bool>,
    pub extra_args: Option<String>,
    /// JVM 优化预设：default / low_spec / big_modpack
    #[serde(default)]
    pub preset: Option<String>,
    /// 全屏启动
    #[serde(default)]
    pub fullscreen: Option<bool>,
    #[serde(default)]
    pub window_width: Option<serde_json::Value>,
    #[serde(default)]
    pub window_height: Option<serde_json::Value>,
    pub isolation: Option<bool>,
}
