// Java 运行时检测与自动管理模块
//
// - model:   JavaInfo / 实例满足状态数据结构
// - detect:  系统扫描（PATH/JAVA_HOME/注册表/常见目录/启动器目录）与版本规则表
// - install: Adoptium Temurin 自动下载（走下载引擎）与 zip 解压

pub mod detect;
pub mod install;
pub mod model;
