// Java 检测数据模型
//
// JavaInfo：单个可用运行时（探测自 java -XshowSettings:properties -version 输出）；
// DetectJavaResult：detect_java 命令的完整返回（系统 Java 列表 + 各实例满足状态）。

use serde::Serialize;

/// 检测到的 Java 运行时
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaInfo {
    /// java.exe 绝对路径
    pub path: String,
    /// 主版本号（8 / 17 / 21 …）
    pub version: u32,
    /// 位数 / 架构：x64 / x86 / aarch64
    pub arch: String,
    /// 发行版（java.vendor，如 Temurin / Oracle Corporation / Microsoft）
    pub vendor: String,
    /// 是否为用户手动登记的路径（settings.json customJavaPaths 命中；
    /// 手动条目在 Java 管理页可移除登记）
    #[serde(default)]
    pub custom: bool,
}

/// 实例的 Java 满足状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceJavaStatus {
    pub instance_id: String,
    pub instance_name: String,
    pub mc_version: String,
    /// 该 MC 版本要求的最低 Java 主版本
    pub required_java: u32,
    /// 实例手动指定的 Java 路径（None = 启动时自动选择）
    pub assigned_path: Option<String>,
    /// 指定路径在系统检测中对应的版本
    pub assigned_version: Option<u32>,
    /// 指定时：版本是否达标；未指定时：系统中是否存在满足要求的 Java
    pub satisfied: bool,
}

/// detect_java 命令返回
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectJavaResult {
    pub javas: Vec<JavaInfo>,
    pub instances: Vec<InstanceJavaStatus>,
}
