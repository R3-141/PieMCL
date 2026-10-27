<img width="1535" height="500" alt="mcl" src="https://github.com/user-attachments/assets/18553c84-1136-4124-beec-ba4b96b13bf0" />
PieMCL 是一款免费开源的 **Minecraft: Java Edition** Windows 启动器，基于 Tauri v2（Rust + React）构建。

> 非官方 Minecraft 产品，未获得 Mojang 或 Microsoft 认可，与之无关联。

中文 | [English](./README_en.md) | [免责声明](./DISCLAIMER.md)

## 功能

### 实例管理
- 多实例相互隔离，每个实例拥有独立的游戏目录、版本与加载器
- 一键安装官方 Mojang 版本（正式版与快照版）及模组加载器（Fabric / Quilt / Forge / NeoForge，Forge 可选搭配 OptiFine）
- **外部实例自动导入**：把启动器放在 `.minecraft` 同级目录，其他启动器（HMCL、PCL、官方启动器）创建的实例会被自动识别并导入——零侵入，不修改任何原有文件，原启动器仍可正常使用
- 实例复制、重命名、删除（移入回收站），以及每实例独立设置（内存、JVM 优化预设、附加 JVM 参数、窗口尺寸 / 全屏、自定义 Java）

<img width="1377" height="877" alt="QQ_1792943088571" src="https://github.com/user-attachments/assets/ef6e7479-875d-44e5-b87d-6bc2077b54ef" />


### 模组市场与资源管理
- 内置浏览并一键安装 **模组**（Modrinth 与 CurseForge）、**数据包**、**光影包** 与 **资源包**（Modrinth），支持按下载量 / 更新时间 / 相关度排序及版本与加载器筛选
- 安装前自动解析完整依赖链，逐文件下载进度、更新检查，支持全窗口拖放安装
- 模组启用 / 禁用（基于重命名，不移动文件）、实例回收站（可恢复、可设置保留天数）、缺失依赖提醒
- 整合包导入：支持 `.mrpack`（Modrinth）与 CurseForge `manifest.json` 压缩包——自动安装加载器、部署全部整合包文件与 `overrides/`（配置、资源包、光影包等）；实例也可导出为 `.mrpack` 与他人分享

<img width="1377" height="877" alt="QQ_1792943126083" src="https://github.com/user-attachments/assets/e6d22e4f-afd1-4d9a-af82-10ddd8f3f922" />


### 游戏启动
- 严格遵循 Mojang 官方命令行规范：条件式 JVM 参数、natives 解压，同时完整兼容新旧两种版本格式（`minecraftArguments` 与 `arguments`）
- 启动前自动补全游戏资源（主 jar、libraries、assets）并进行哈希校验
- 实时日志查看器、崩溃摘要、进程结束 / 重启控制；游戏未退出时关闭启动器有守护确认

### 下载引擎
- 多线程并发下载，大文件自动分块（并行 Range 请求），支持断点续传与 SHA-1 / SHA-256 完整性校验
- 官方源与社区镜像（BMCLAPI、MCIMirror）自动候选轮换，并针对不同源的限流特性调优并发
- 全局 / 单主机并发数可配置，支持代理

### Java 管理
- 自动检测已安装的 Java 运行时：注册表、`PATH`、`JAVA_HOME`、常见发行版目录以及官方启动器自带运行时（对非 ASCII 安装路径容错）
- 按游戏版本一键自动下载所需的 Java 大版本（Eclipse Temurin），支持手动登记自定义 Java 路径

<img width="1377" height="877" alt="QQ_1792943155512" src="https://github.com/user-attachments/assets/b1e22dab-cb19-4bc6-9f2d-33d4463435f7" />


### 账户与皮肤
- **离线账户**：自定义玩家 ID，本地皮肤注入（内置本地 Yggdrasil 会话服务）
- **Microsoft 正版账户**：通过官方 OAuth 2.0 设备码流程登录，可经由官方 Minecraft Services API 管理皮肤与披风
- 主页 3D 皮肤预览，支持披风显示 / 隐藏切换

<img width="1377" height="877" alt="QQ_1792943194681" src="https://github.com/user-attachments/assets/dc07c6e6-ee63-437f-871d-6baeb60c0b6b" />


### 联机与服务器
- 服务器浏览：实时状态（MOTD、在线人数、玩家列表、SRV 记录解析），一键把服务器导入任意实例
- 内置樱花内网穿透（SakuraFrp）隧道管理：下载 `frpc`、创建与管理隧道、一键开启端口转发，全程无需离开启动器

<img width="1371" height="928" alt="QQ_1792943232571" src="https://github.com/user-attachments/assets/b8c91b22-cacf-4bbd-ac20-c878f818dc3e" />
<img width="1371" height="928" alt="QQ_1792943267717" src="https://github.com/user-attachments/assets/852fe32a-5258-4d85-91fe-ed6df8788b7a" />

### 媒体与工具
- 实例截图查看器：网格预览、大图查看、复制到剪贴板、打开文件位置、删除
- 存档管理：自动生成缩略图、备份为 zip 到任意位置、定时删除二次确认
- 下载面板：所有任务支持暂停 / 继续 / 取消 / 重试；启动器数据目录完全便携

<img width="1377" height="877" alt="QQ_1792943314615" src="https://github.com/user-attachments/assets/21e294f6-a180-4258-87da-548b8100a976" />
<img width="1377" height="877" alt="QQ_1792943366534" src="https://github.com/user-attachments/assets/e04b910a-8ef2-4683-bbc9-e2982e493421" />


### 外观
- 浅色 / 深色 / 跟随系统主题、自定义主色、Acrylic 窗口磨砂、自定义背景图（应用内裁剪、模糊与遮罩调节）

<img width="1568" height="972" alt="QQ_1792943410500" src="https://github.com/user-attachments/assets/3523ecd3-aa24-4815-83b0-98b43c705097" />


## 安全与隐私

- Microsoft 登录采用官方设备码流程，在用户系统浏览器中完成——启动器不接触、不存储 Microsoft 密码
- 令牌（OAuth 刷新令牌与 Minecraft 访问令牌）**仅**保存在用户本机的启动器应用数据目录中，除直接传输至官方 Microsoft / Mojang 端点外不发送到任何地方
- 无遥测、无数据上报——所有流量直达官方 Mojang / Microsoft / Modrinth / CurseForge 端点（以及用户自行配置的镜像）

## 技术栈

- **后端：** Rust（Tokio 异步运行时）、Tauri v2
- **前端：** React 19、TypeScript、Tailwind CSS

## 从源码构建

前置要求：[Node.js](https://nodejs.org/) 20+、[pnpm](https://pnpm.io/)、[Rust](https://rustup.rs/)（Windows 下为 MSVC 工具链）。

```bash
pnpm install
pnpm tauri dev     # 开发构建
pnpm tauri build   # 发行构建 → src-tauri/target/release/piemcl.exe
```

启动器为单个便携可执行文件，放在 `.minecraft` 同级目录即可运行（也可以放在任意位置——其他启动器创建的现有实例会被自动识别并导入）。

## 许可证

本项目基于 [GNU General Public License v3.0](./LICENSE) 开源。

Copyright © 2026 R3_141 and contributors.
