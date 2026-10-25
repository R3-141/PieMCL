// 加载器安装支持
//
// 目前实现 Forge / NeoForge 的现代安装管线（install_profile spec=1，覆盖 1.17+
// 全部 Forge 与 NeoForge 1.20.2+ 及 NeoForge 1.20.1 特例）：
//   installer.jar 下载 → 解析 install_profile.json / version.json →
//   解压内置 maven/ 库 → 下载 processor 依赖库 → 逐个执行 processors
//   （MCP 数据解包 / Mojang 映射下载 / 映射合并 / jar 拆分 / 重映射 / binpatch）→
//   校验产物 → 生成 patch 后主 jar → 合并 version.json 写回实例。
//
// 限制：install_profile spec=0（Forge 1.16.5 及更早，versionInfo 内嵌 + universal
// 解压式安装）暂不支持，入口处给出明确错误。

pub mod installer;
