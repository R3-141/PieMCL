// 业务领域类型：Rust 侧 single source of truth 的前端镜像
// 字段命名与 Rust serde(rename_all = "camelCase") 输出一致

/** 模组加载器类型（OptiFine 非加载器，仅作 Forge 附加项） */
export type LoaderType = 'vanilla' | 'forge' | 'fabric' | 'neoforge' | 'quilt'

/** 实例摘要（列表卡片用） */
export interface InstanceSummary {
  id: string
  name: string
  icon: string | null
  mcVersion: string
  loaderType: string
  loaderVersion: string | null
  /** 搭配的 OptiFine 版本（仅 Forge 实例可能有） */
  optifineVersion: string | null
  /** 最后启动时间（epoch 秒）；null = 从未启动 */
  lastLaunchedAt: number | null
}

/** JVM 启动参数设置 */
export interface JvmSettings {
  minMemoryMb: number
  maxMemoryMb: number
  extraArgs: string
  /** JVM 优化预设：default / low_spec / big_modpack */
  preset: string
  windowWidth: number | null
  windowHeight: number | null
  /** 全屏启动（优先于自定义宽高） */
  fullscreen: boolean
}

/** 实例详情（编辑面板用） */
export interface InstanceDetail extends InstanceSummary {
  createdAt: number
  javaPath: string | null
  isolation: boolean
  jvm: JvmSettings
}

/**
 * 实例设置补丁；不传字段 = 不修改。
 * javaPath / 窗口尺寸支持 null 语义 = 恢复自动（对应 Rust 双层 Option）。
 */
export interface UpdateInstancePatch {
  name?: string
  javaPath?: string | null
  minMemoryMb?: number
  maxMemoryMb?: number
  extraArgs?: string
  preset?: string
  fullscreen?: boolean
  windowWidth?: number | null
  windowHeight?: number | null
  isolation?: boolean
}

/** MC 版本清单条目 */
export interface McVersionEntry {
  id: string
  versionType: string
  releaseTime: string
}

/** 创建实例进度事件 payload（instance-create-progress） */
export interface CreateProgress {
  stage: 'fetch_manifest' | 'fetch_version' | 'write' | 'done'
  message: string
}

/** 检测到的 Java 运行时（detect_java） */
export interface JavaInfo {
  path: string
  /** 主版本号（8 / 17 / 21 …） */
  version: number
  /** 位数 / 架构：x64 / x86 / aarch64 */
  arch: string
  /** 发行版（Temurin / Oracle Corporation / Microsoft …） */
  vendor: string
}

/** 实例的 Java 满足状态 */
export interface InstanceJavaStatus {
  instanceId: string
  instanceName: string
  mcVersion: string
  /** 该 MC 版本要求的最低 Java 主版本 */
  requiredJava: number
  /** 实例手动指定的 Java 路径；null = 启动时自动选择 */
  assignedPath: string | null
  /** 指定路径在系统检测中对应的版本 */
  assignedVersion: number | null
  /** 指定时：版本是否达标；未指定时：系统中是否存在满足要求的 Java */
  satisfied: boolean
}

/** detect_java 命令返回 */
export interface DetectJavaResult {
  javas: JavaInfo[]
  instances: InstanceJavaStatus[]
}

/** 下载进度事件 payload（download-progress，Rust 引擎 200ms 节流推送） */
export interface DownloadProgress {
  taskId: string
  /** 任务类型：resources / loader / java */
  kind: string
  /** 关联实例或资源锁键（Java 下载为 java:{major}） */
  instanceId: string | null
  /** running / paused / completed / failed / cancelled */
  state: string
  /** 正在下载的文件显示名 */
  currentFile: string | null
  /** 已完成文件数（含本地校验跳过） */
  completedFiles: number
  totalFiles: number
  downloadedBytes: number
  /** 预估总字节数；0 = 未知（按文件数展示进度） */
  totalBytes: number
  /** 实时速度（字节/秒） */
  bytesPerSecond: number
  /** 失败原因（仅 failed 状态） */
  error: string | null
}

/** OptiFine 版本条目（list_optifine_forge） */
export interface OptiFineEntry {
  optifineType: string
  patch: string
  filename: string
  minForge: string | null
  compatible: boolean
}

/** 全局设置（get_settings / save_settings） */
export interface AppSettings {
  /** 镜像源列表（按序尝试；仅改写 Mojang 官方域名） */
  mirrors: string[]
  /** 下载源策略：auto（官方直连优先，失败轮换镜像）/ official（仅官方）/ mirror（镜像优先） */
  downloadSource: string
  /** 全局并发下载连接上限（1–256，修改需重启生效） */
  globalConcurrency: number
  /** 单主机并发上限（1–64，修改需重启生效） */
  perHostConcurrency: number
  /** 离线模式的默认玩家名（历史遗留，启动已改用账户系统） */
  defaultPlayerName: string
  /** 模组回收站保留天数（0 = 删除即永久移除） */
  trashRetentionDays: number
  /** CurseForge API Key（留空仅影响 CurseForge） */
  curseforgeApiKey: string
  /** 网络代理模式：none（直连）/ system（跟随系统）/ manual（手动地址） */
  proxyMode: string
  /** 手动代理地址（proxyMode = manual 时使用） */
  proxyUrl: string
  /** 自定义背景图文件名（空 = 未启用；文件在数据根 background/ 目录） */
  backgroundImage: string
  /** 背景图模糊程度（px，0–40） */
  backgroundBlur: number
  /** 背景图遮罩暗度（%，0–70） */
  backgroundDim: number
  /** 背景启用时的界面明暗：auto（跟随主题）/ dark / light */
  backgroundTheme: string
}

/** 正版 profile 的单张皮肤（缓存） */
export interface SkinEntry {
  id: string
  /** ACTIVE = 当前皮肤 */
  state: string
  url: string
  /** classic（粗壮）/ slim（纤细） */
  variant: string
  alias?: string | null
}

/** 正版 profile 的单件披风（缓存） */
export interface CapeEntry {
  id: string
  state: string
  alias?: string | null
  url: string
}

/** 离线账户注入皮肤元数据（PNG 存本地 skins/ 目录） */
export interface OfflineSkinMeta {
  /** 来源正版玩家名 */
  sourceName: string
  /** classic / slim */
  variant: string
  /** 抓取时间（epoch 秒） */
  fetchedAt: number
}

/** 账户视图（list_accounts；token 等敏感字段不出 Rust） */
export interface AccountView {
  id: string
  /** offline / microsoft */
  kind: string
  name: string
  uuid: string
  /** 创建时间（epoch 秒） */
  createdAt: number
  /** 正版 profile 缓存的皮肤 / 披风（离线账户为空） */
  skins: SkinEntry[]
  capes: CapeEntry[]
  activeCapeId: string | null
  /** 离线账户注入皮肤元数据（批次 D） */
  offlineSkin: OfflineSkinMeta | null
}

/** 账户列表结果 */
export interface AccountListResult {
  accounts: AccountView[]
  activeId: string | null
}

/** ms-login-state 事件 payload（微软设备码登录流程） */
export interface MsLoginStatePayload {
  /** code=展示设备码 pending=认证中 success / expired / denied / failed */
  stage: string
  /** code 阶段：设备码与授权页 URL */
  userCode?: string
  verifyUrl?: string
  /** code 阶段：Rust 侧是否已自动复制到剪贴板 */
  copied?: boolean
  message?: string
}

/** java-installed 事件 payload */
export interface JavaInstalledPayload {
  major: number
  path: string
}

/** java-install-failed 事件 payload */
export interface JavaInstallFailedPayload {
  major: number
  error: string
}

/** launch-state 事件 payload（启动状态机阶段推进） */
export interface LaunchStatePayload {
  instanceId: string
  stage:
    | 'checking_java'
    | 'checking_resources'
    | 'downloading'
    | 'extracting_natives'
    | 'building'
    | 'starting'
    | 'running'
    | 'exited'
    | 'failed'
  message: string
  /** running 阶段附带进程 ID */
  pid?: number
}

/** launch-log 事件 payload（逐行日志） */
export interface LaunchLogPayload {
  instanceId: string
  stream: 'stdout' | 'stderr'
  line: string
}

/** launch-exited 事件 payload */
export interface LaunchExitedPayload {
  instanceId: string
  code: number | null
  /** 可读崩溃摘要；正常退出为 null */
  summary: string | null
}

/** 运行中实例状态（list_running） */
export interface RunningState {
  instanceId: string
  pid: number
  startedAt: number
  memoryBytes: number | null
  uptimeSecs: number
}

/** 整合包预览（pick_modpack） */
export interface PackPreview {
  kind: string
  name: string
  mcVersion: string
  loaderType: string
  loaderVersion: string | null
  modCount: number
  /** 整合包文件路径（导入时回传） */
  path: string
}

/** pack-import-progress 事件 payload */
export interface PackImportProgress {
  stage: string
  message: string
  current: number | null
  total: number | null
}

/** 导出整合包：单个可勾选内容（key 回传给 export_modpack；文件项为相对 minecraft/ 的路径） */
export interface ExportItem {
  key: string
  label: string
  hint: string | null
  defaultChecked: boolean
}

/** 导出整合包：内容分组 */
export interface ExportGroup {
  key: string
  label: string
  /** 组主体恒选（游戏本体 / Mod），勾选树仅控制附属内容 */
  required: boolean
  items: ExportItem[]
}

/** 模组来源信息（mods.json 记录；无来源 = 外部放入的文件） */
export interface ModSource {
  origin: 'modrinth' | 'curseforge' | 'local'
  projectId?: string | null
  versionId?: string | null
  versionNumber?: string | null
  sha1?: string | null
  downloadUrl?: string | null
}

/** 单个模组（list_mods） */
export interface ModInfo {
  /** 文件名（含 .disabled 后缀，启停凭据） */
  fileName: string
  enabled: boolean
  fileSize: number
  /** 元数据 id；解析失败为 null（降级为文件名展示） */
  modId: string | null
  name: string
  version: string | null
  /** 元数据自报加载器：fabric / forge / neoforge / null */
  loader: string | null
  /** 必需前置依赖 id */
  depends: string[]
  /** 缺失的前置（depends − 已启用模组 id） */
  missingDeps: string[]
  source?: ModSource | null
}

/** 回收站条目（list_trash） */
export interface TrashItem {
  trashName: string
  originalName: string
  /** 删除时间（epoch 秒） */
  deletedAt: number
  fileSize: number
}

/** 拖放 / 从文件安装结果（三类分开提示） */
export interface LocalInstallResult {
  installed: string[]
  skipped: string[]
  failed: string[]
}

/** Modrinth / CurseForge 搜索结果条目 */
export interface SearchHit {
  projectId: string
  slug: string
  title: string
  description: string
  downloads: number
  iconUrl: string | null
  author: string | null
  /** modrinth / curseforge */
  origin: string
  /** 展示分类（含加载器与内容分类） */
  displayCategories: string[]
  /** 最近更新时间（ISO 8601） */
  dateModified: string | null
}

/** Modrinth 版本内的可下载文件 */
export interface VersionFile {
  url: string
  filename: string
  primary: boolean
  size: number
  sha1: string | null
}

/** Modrinth 版本详情 */
export interface OnlineVersion {
  id: string
  projectId: string
  versionNumber: string
  versionType: string
  gameVersions: string[]
  loaders: string[]
  dependencies: { versionId: string | null; projectId: string | null; dependencyType: string }[]
  files: VersionFile[]
}

/** 计划安装的单个文件（主文件或依赖） */
export interface PlanDep {
  projectId: string
  slug: string
  title: string
  versionId: string
  versionNumber: string
  file: VersionFile
  alreadyInstalled: boolean
  /** modrinth / curseforge */
  origin: string
}

/** 安装计划（两段式：先展示计划再确认） */
export interface InstallPlan {
  main: PlanDep
  deps: PlanDep[]
  /** 无法自动定位版本的必需依赖（提示手动处理） */
  unknownDeps: string[]
  /** 可选依赖（作者标记 optional 的推荐项，默认不勾选） */
  recommended: PlanDep[]
}

/** 可更新条目（check_mod_updates） */
export interface UpdateInfo {
  /** modrinth / curseforge */
  origin: string
  fileName: string
  projectId: string
  slug: string
  title: string
  currentVersionNumber: string
  newVersionNumber: string
  /** 统一安装目标（mr:... / cf:...），直接传给 confirmModInstall */
  newVersionId: string
}

/** 游戏配置模板（启动器全局保存 + 应用到实例同构）。
 * 字段缺省 = 未设置，应用时跳过 —— 只同步明确设置过的项。 */
export interface GameConfigTemplate {
  /** 视场角（度数 30–110，UI 层值；写入时按版本换算存储格式） */
  fov?: number
  guiScale?: number
  /** 亮度（0–1） */
  brightness?: number
  renderDistance?: number
  maxFps?: number
  fullscreen?: boolean
  /** 鼠标灵敏度（0–1，0.5 = 游戏默认） */
  mouseSensitivity?: number
  autoJump?: boolean
  /** 主音量（0–1） */
  masterVolume?: number
  /** 键位：action id（如 key.forward）→ 1.13+ 符号名（仅收录表内条目生效） */
  keys?: Record<string, string>
}

/** 收藏服务器条目（servers.json，启动器级数据） */
export interface ServerEntry {
  id: string
  /** 备注名（缺省展示地址） */
  name: string
  /** 地址（host 或 host:port） */
  address: string
}

/** 服务器 ping 结果（Server List Ping；MOTD 已转为 § 颜色码字符串） */
export interface ServerPing {
  /** 延迟（毫秒） */
  latencyMs: number
  versionName: string
  protocol: number
  playersOnline: number
  playersMax: number
  /** base64 PNG data URL（服务器未提供时为 null） */
  favicon: string | null
  motd: string
  /** 是否为 Forge / 模组服务器 */
  forge: boolean
  /** 在线玩家名单（players.sample；空 = 服务器未提供） */
  playerSample: string[]
}

/** 截图文件（实例 screenshots 目录） */
export interface ScreenshotInfo {
  name: string
  size: number
  /** 修改时间（Unix 毫秒） */
  modified: number
}

/** 存档（实例 saves 目录，含 level.dat 的目录；hasIcon 标记是否有 icon.png 缩略图） */
export interface SaveInfo {
  name: string
  size: number
  /** 修改时间（Unix 毫秒） */
  modified: number
  hasIcon: boolean
}

/** 联机（樱花内网穿透）状态 */
export interface NatFrpStatus {
  /** frpc 可执行文件是否就绪 */
  frpcReady: boolean
  /** 实际使用的 frpc 路径 */
  frpcPath: string
  /** frpc 路径是否为用户手动指定的 */
  frpcCustom: boolean
  /** 是否已配置访问密钥 */
  accessKeySet: boolean
  /** 访问密钥明文（回显输入框） */
  accessKey: string
  /** 已保存的隧道 ID（回显输入框） */
  tunnelId: string
  /** frpc 是否运行中 */
  running: boolean
}

/** 隧道连接地址（已解析 IPv4 直连形式） */
export interface NatFrpAddr {
  /** 域名:端口 / IP:端口 形式的原始地址 */
  address: string
  /** IPv4 直连地址（ip:端口，可能为空 = 解析失败） */
  ipv4: string[]
}

/** 樱花隧道（列表展示） */
export interface NatFrpTunnel {
  id: number
  name: string
  tunnelType: string
  nodeId: number
  nodeName: string
  online: boolean
  banned: boolean
  statusReason: string
  localIp: string
  localPort: number
  remote: string
}

/** 樱花节点（新建隧道选择用） */
export interface NatFrpNode {
  id: number
  name: string
  host: string
  description: string
  /** 允许创建隧道（满载时为 false） */
  canCreate: boolean
  allowUdp: boolean
  /** 允许穿透公开游戏服务器（flag&2048 或名称含"多线"） */
  allowPublicGame: boolean
  offline: boolean
  isPrivate: boolean
}

/** 联机配置（保存载荷） */
export interface NatFrpConfig {
  accessKey: string
  frpcPath?: string | null
}
