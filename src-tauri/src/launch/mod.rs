// 启动执行引擎模块
//
// - rules:   启动参数条件规则评估（os / features）
// - args:    启动命令构建（JVM/Game 参数、占位符、classpath、Java 选择）
// - natives: natives classifiers 解压（LWJGL2 风格库）
// - process: 进程 spawn / 日志流 / 强制结束 / 内存查询

pub mod args;
pub mod natives;
pub mod process;
pub mod rules;
