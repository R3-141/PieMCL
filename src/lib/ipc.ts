// Tauri IPC 封装：command 调用与事件监听
//
// Rust command 参数约定 camelCase（Tauri v2 自动映射 snake_case → camelCase）；
// 所有业务数据由此进出，前端不持有业务状态。

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AccountListResult,
  AccountView,
  AuthlibLoginResult,
  AuthlibServerMeta,
  AuthlibSkinData,
  AppSettings,
  CreateProgress,
  DetectJavaResult,
  DownloadProgress,
  InstallPlan,
  ExportGroup,
  InstanceDetail,
  InstanceSummary,
  GameConfigTemplate,
  JavaInfo,
  ServerEntry,
  ServerPing,
  NatFrpStatus,
  NatFrpAddr,
  NatFrpTunnel,
  NatFrpNode,
  ScreenshotInfo,
  SaveInfo,
  JavaInstallFailedPayload,
  JavaInstalledPayload,
  LaunchExitedPayload,
  LaunchLogPayload,
  LaunchStatePayload,
  LocalInstallResult,
  McVersionEntry,
  ModInfo,
  MsLoginStatePayload,
  AuthlibOauthStatePayload,
  UpdateCheckResult,
  OnlineVersion,
  OptiFineEntry,
  PackImportProgress,
  PackPreview,
  RunningState,
  SearchHit,
  TrashItem,
  UpdateInstancePatch,
  UpdateInfo,
} from '../types/domain'

export interface CreateInstanceArgs {
  name: string
  mcVersion: string
  loaderType?: string
  loaderVersion?: string
  /** 仅 Forge 可搭配 OptiFine */
  optifineVersion?: string
}

export const ipc = {
  listInstances: () => invoke<InstanceSummary[]>('list_instances'),

  getInstance: (id: string) => invoke<InstanceDetail>('get_instance', { id }),

  createInstance: (args: CreateInstanceArgs) =>
    invoke<InstanceSummary>('create_instance', {
      name: args.name,
      mcVersion: args.mcVersion,
      loaderType: args.loaderType ?? null,
      loaderVersion: args.loaderVersion ?? null,
      optifineVersion: args.optifineVersion ?? null,
    }),

  deleteInstance: (id: string, permanent: boolean) =>
    invoke<void>('delete_instance', { id, permanent }),

  duplicateInstance: (id: string, newName: string, copySaves: boolean) =>
    invoke<InstanceSummary>('duplicate_instance', { id, newName, copySaves }),

  updateInstanceSettings: (id: string, patch: UpdateInstancePatch) =>
    invoke<InstanceDetail>('update_instance_settings', { id, patch }),

  openInstanceFolder: (id: string) => invoke<void>('open_instance_folder', { id }),

  listMcVersions: (showSnapshots: boolean, refresh: boolean) =>
    invoke<McVersionEntry[]>('list_mc_versions', { showSnapshots, refresh }),

  listLoaderVersions: (loaderType: string, mcVersion: string) =>
    invoke<string[]>('list_loader_versions', { loaderType, mcVersion }),

  /** 补全实例游戏资源（client.jar / libraries / assets），返回 taskId */
  installGameResources: (instanceId: string) =>
    invoke<string>('install_game_resources', { instanceId }),

  /** 安装加载器（Fabric / Quilt；Forge 留待后续），不传参沿用实例 loader.json，返回 taskId */
  installLoader: (instanceId: string, loaderType?: string, loaderVersion?: string) =>
    invoke<string>('install_loader', {
      instanceId,
      loaderType: loaderType ?? null,
      loaderVersion: loaderVersion ?? null,
    }),

  /** 暂停 / 继续 / 取消下载任务 */
  pauseDownload: (taskId: string) => invoke<void>('pause_download', { taskId }),
  resumeDownload: (taskId: string) => invoke<void>('resume_download', { taskId }),
  cancelDownload: (taskId: string) => invoke<void>('cancel_download', { taskId }),

  /** 重试失败 / 已取消的下载任务（进度覆盖原任务行） */
  retryDownload: (taskId: string) => invoke<void>('retry_download', { taskId }),

  /** 检测系统 Java 与各实例满足状态 */
  detectJava: () => invoke<DetectJavaResult>('detect_java'),

  /** 从 Adoptium 下载指定主版本 JRE，返回 taskId */
  downloadJava: (majorVersion: number) =>
    invoke<string>('download_java', { majorVersion }),

  /** 手动登记 Java 路径（java.exe 或其所在目录），探测成功后持久化并返回识别信息 */
  addCustomJava: (path: string) => invoke<JavaInfo>('add_custom_java', { path }),

  /** 移除手动登记的 Java 路径（仅取消登记，不删除文件；调用后需重新检测） */
  removeCustomJava: (path: string) => invoke<void>('remove_custom_java', { path }),

  /** 读取游戏配置模板（启动器全局；文件缺失返回空模板） */
  loadGameConfig: () => invoke<GameConfigTemplate>('load_game_config'),

  /** 保存游戏配置模板到启动器数据目录 */
  saveGameConfig: (template: GameConfigTemplate) =>
    invoke<void>('save_game_config', { template }),

  /** 把模板写入指定实例的 options.txt（原位改写，版本键名 / 键码自动转换） */
  writeGameConfig: (instanceId: string, template: GameConfigTemplate) =>
    invoke<void>('write_game_config', { instanceId, template }),

  /** 收藏服务器列表（启动器级） */
  listServers: () => invoke<ServerEntry[]>('list_servers'),

  /** 添加收藏服务器（name 缺省用地址），返回新条目 */
  addServer: (name: string | null, address: string) =>
    invoke<ServerEntry>('add_server', { name, address }),

  /** 修改收藏服务器（名称 / 地址） */
  updateServer: (id: string, name: string, address: string) =>
    invoke<void>('update_server', { id, name, address }),

  /** 删除收藏服务器 */
  removeServer: (id: string) => invoke<void>('remove_server', { id }),

  /** Server List Ping：查询服务器状态（失败为 Err 中文原因） */
  pingServer: (address: string) => invoke<ServerPing>('ping_server', { address }),

  /** 联机（樱花内网穿透）状态 */
  natfrpStatus: () => invoke<NatFrpStatus>('natfrp_status'),

  /** 保存联机配置（访问密钥 + 隧道 ID + 可选 frpc 路径覆盖） */
  natfrpSaveConfig: (accessKey: string, tunnelId: string, frpcPath?: string | null) =>
    invoke<void>('natfrp_save_config', { accessKey, tunnelId, frpcPath: frpcPath ?? null }),

  /** 用系统浏览器打开樱花面板页面（隧道列表 / 访问密钥等直达入口） */
  natfrpOpenPage: (url: string) => invoke<void>('natfrp_open_page', { url }),

  /** 手动选择 frpc.exe（校验后写入配置），返回落位路径 */
  natfrpPickFrpc: () => invoke<string>('natfrp_pick_frpc'),

  /** 自动下载 frpc（进度走下载面板，互斥键 natfrp:frpc） */
  natfrpDownloadFrpc: () => invoke<void>('natfrp_download_frpc'),

  /** 启动隧道（frpc -f 密钥:隧道ID；日志经 natfrp-log 事件推送） */
  natfrpStart: (tunnelId: string) => invoke<void>('natfrp_start', { tunnelId }),

  /** 停止穿透（强杀 frpc） */
  natfrpStop: () => invoke<void>('natfrp_stop'),

  /** 按访问密钥从樱花隧道 API 拉取连接地址（frpc 日志不含远程地址） */
  natfrpFetchAddr: () => invoke<NatFrpAddr[]>('natfrp_fetch_tunnel'),

  /** 隧道列表（合并节点名；隐藏 wol 类型） */
  natfrpListTunnels: () => invoke<NatFrpTunnel[]>('natfrp_list_tunnels'),

  /** 节点列表（新建隧道选择用） */
  natfrpListNodes: () => invoke<NatFrpNode[]>('natfrp_list_nodes'),

  /** 创建隧道（tcp/udp 的 remote 留空 = 服务器随机分配），返回新隧道 ID */
  natfrpCreateTunnel: (p: {
    name: string
    tunnelType: string
    nodeId: number
    localIp: string
    localPort: number
    remote?: string
    note?: string
  }) =>
    invoke<number>('natfrp_create_tunnel', {
      name: p.name,
      tunnelType: p.tunnelType,
      nodeId: p.nodeId,
      localIp: p.localIp,
      localPort: p.localPort,
      remote: p.remote ?? null,
      note: p.note ?? null,
    }),

  /** 删除隧道，返回是否确实被删除 */
  natfrpDeleteTunnel: (tunnelId: number) =>
    invoke<boolean>('natfrp_delete_tunnel', { tunnelId }),

  /** 把收藏服务器全部导入实例的服务器列表（servers.dat，按地址去重合并） */
  exportServersToInstance: (instanceId: string) =>
    invoke<{ imported: number; skipped: number }>('export_servers_to_instance', {
      instanceId,
    }),

  // ---- 截图与存档（实例资源管理页） ----

  /** 截图列表（按修改时间倒序） */
  listScreenshots: (instanceId: string) =>
    invoke<ScreenshotInfo[]>('list_screenshots', { instanceId }),

  /** 读取截图 data URL（缩略图 / 预览） */
  readScreenshot: (instanceId: string, name: string) =>
    invoke<string>('read_screenshot', { instanceId, name }),

  /** 删除截图（移入回收站） */
  deleteScreenshot: (instanceId: string, name: string) =>
    invoke<void>('delete_screenshot', { instanceId, name }),

  /** 在资源管理器中高亮截图文件 */
  revealScreenshot: (instanceId: string, name: string) =>
    invoke<void>('reveal_screenshot', { instanceId, name }),

  /** 复制截图到系统剪贴板（位图） */
  copyScreenshotToClipboard: (instanceId: string, name: string) =>
    invoke<void>('copy_screenshot_to_clipboard', { instanceId, name }),

  /** 存档列表（按修改时间倒序） */
  listSaves: (instanceId: string) => invoke<SaveInfo[]>('list_saves', { instanceId }),

  /** 读取存档缩略图 data URL（无 icon.png 时为 null） */
  readSaveIcon: (instanceId: string, name: string) =>
    invoke<string | null>('read_save_icon', { instanceId, name }),

  /** 删除存档（整个目录移入回收站） */
  deleteSave: (instanceId: string, name: string) =>
    invoke<void>('delete_save', { instanceId, name }),

  /** 备份存档（弹保存对话框选位置 → zip 打包），返回保存路径 */
  backupSave: (instanceId: string, name: string) =>
    invoke<string>('backup_save', { instanceId, name }),

  /** 打开实例的存档目录 */
  openSavesDir: (instanceId: string) => invoke<void>('open_saves_dir', { instanceId }),

  /** 读取 / 保存全局设置（保存返回规范化后的设置） */
  getSettings: () => invoke<AppSettings>('get_settings'),
  saveSettings: (settings: AppSettings) =>
    invoke<AppSettings>('save_settings', { settings }),

  /** 数据目录路径 / 在文件管理器中打开 */
  getDataDir: () => invoke<string>('get_data_dir'),
  openDataDir: () => invoke<void>('open_data_dir'),
  openExternal: (url: string) => invoke<void>('open_external', { url }),

  /** 在文件管理器中定位文件 / 目录（Java 管理等页面用） */
  revealPath: (path: string) => invoke<void>('reveal_path', { path }),

  /** 检查更新（force=true 为设置页手动检查，忽略跳过版本与启动开关） */
  checkUpdate: (force: boolean) => invoke<UpdateCheckResult>('check_update', { force }),
  /** 清空市场图标缓存，返回删除的文件数 */
  clearMarketCache: () => invoke<number>('clear_market_cache'),

  /** 保存裁剪后的背景图（JPEG 字节 + 未裁剪原图），返回规范化设置 */
  saveBackgroundImage: (bytes: ArrayBuffer, original: ArrayBuffer, ext: string) =>
    invoke<AppSettings>('save_background_image', {
      bytes: Array.from(new Uint8Array(bytes)),
      original: Array.from(new Uint8Array(original)),
      ext,
    }),

  /** 读取背景图 data URL；未启用或文件缺失返回 null */
  readBackgroundImage: () => invoke<string | null>('read_background_image'),

  /** 移除自定义背景，返回规范化设置 */
  removeBackgroundImage: () => invoke<AppSettings>('remove_background_image'),

  /** 弹出文件对话框选择背景图，返回 data URL（取消返回 null） */
  pickBackgroundImage: () => invoke<string | null>('pick_background_image'),

  /** 读取未裁剪原图 data URL（重新裁剪用；不存在返回 null） */
  readBackgroundOriginal: () => invoke<string | null>('read_background_original'),

  /** Fabric / Quilt 附加组件一键安装（Fabric API / Iris+Sodium），返回安装的文件名列表 */
  installFabricAddons: (instanceId: string, withApi: boolean, withIrisSodium: boolean) =>
    invoke<string[]>('install_fabric_addons', {
      instanceId,
      withApi,
      withIrisSodium,
    }),

  /** OptiFine 版本列表（含与目标 Forge 构建的兼容性标记） */
  listOptifineForge: (mcVersion: string, forgeBuild?: string) =>
    invoke<OptiFineEntry[]>('list_optifine_forge', {
      mcVersion,
      forgeBuild: forgeBuild ?? null,
    }),

  /** 安装 OptiFine 到 Forge 实例（jar 放入 mods 目录），返回文件名 */
  installOptifine: (instanceId: string, optifineType: string, patch: string) =>
    invoke<string>('install_optifine', { instanceId, optifineType, patch }),

  /** 启动实例（立即返回，进度与结果经 launch-* 事件推送） */
  launchInstance: (instanceId: string, serverAddress?: string) =>
    invoke<void>('launch_instance', { instanceId, serverAddress: serverAddress ?? null }),

  /** 强制结束实例的游戏进程 */
  killInstance: (instanceId: string) => invoke<void>('kill_instance', { instanceId }),

  /** 账户列表与激活项（token 不出 Rust） */
  listAccounts: () => invoke<AccountListResult>('list_accounts'),

  /** 添加离线账户（自定义 ID），创建后自动激活 */
  addOfflineAccount: (name: string) => invoke<AccountView>('add_offline_account', { name }),

  /** 切换激活账户 */
  setActiveAccount: (id: string) => invoke<void>('set_active_account', { id }),

  /** 移除账户（激活项自动回落） */
  removeAccount: (id: string) => invoke<void>('remove_account', { id }),

  /** 发起微软设备码登录（立即返回，状态经 ms-login-state 事件推送） */
  loginMicrosoft: () => invoke<void>('login_microsoft'),

  /** 取消当前微软登录流程 */
  cancelMicrosoftLogin: () => invoke<void>('cancel_microsoft_login'),

  /** 预览皮肤站元数据（登录弹窗第一步：确认站点） */
  fetchAuthlibServerMeta: (serverUrl: string) =>
    invoke<AuthlibServerMeta>('fetch_authlib_server_meta', { serverUrl }),

  /** 皮肤站账密登录（多角色时 account 为 null，需 confirmAuthlibProfile 绑定角色） */
  loginAuthlib: (serverUrl: string, username: string, password: string) =>
    invoke<AuthlibLoginResult>('login_authlib', { serverUrl, username, password }),

  /** 多角色账号登录后绑定所选角色 */
  confirmAuthlibProfile: (profileId: string) =>
    invoke<AccountView>('confirm_authlib_profile', { profileId }),

  /** 拉取皮肤站角色当前皮肤（3D 预览用；未设置皮肤返回 null） */
  getAuthlibSkinData: (accountId: string) =>
    invoke<AuthlibSkinData | null>('get_authlib_skin_data', { accountId }),

  /** 发起 LittleSkin OAuth 设备码登录（立即返回，状态经 authlib-oauth-state 事件推送） */
  loginLittleskinOauth: (serverUrl: string) =>
    invoke<void>('login_littleskin_oauth', { serverUrl }),

  /** 取消当前 LittleSkin OAuth 登录流程 */
  cancelLittleskinOauth: () => invoke<void>('cancel_littleskin_oauth'),

  /** 弹出文件对话框选择皮肤 PNG（取消返回 null） */
  pickSkinImage: () => invoke<string | null>('pick_skin_image'),

  /** 应用正版皮肤库中的历史皮肤 */
  applyLibrarySkin: (accountId: string, skinId: string) =>
    invoke<void>('apply_library_skin', { accountId, skinId }),

  /** 切换当前皮肤的模型（variant：slim / classic） */
  switchSkinVariant: (accountId: string, variant: string) =>
    invoke<void>('switch_skin_variant', { accountId, variant }),

  /** 上传本地 PNG 皮肤（variant：slim / classic） */
  uploadSkin: (accountId: string, path: string, variant: string) =>
    invoke<void>('upload_skin', { accountId, path, variant }),

  /** 以 URL 应用皮肤（官方 API 要求 https 地址） */
  applySkinUrl: (accountId: string, url: string, variant: string) =>
    invoke<void>('apply_skin_url', { accountId, url, variant }),

  /** 装备披风 */
  equipCape: (accountId: string, capeId: string) =>
    invoke<void>('equip_cape', { accountId, capeId }),

  /** 脱下披风 */
  unequipCape: (accountId: string) => invoke<void>('unequip_cape', { accountId }),

  /** 抓取正版玩家皮肤并应用到离线账户 */
  fetchAndApplyOfflineSkin: (accountId: string, playerName: string) =>
    invoke<void>('fetch_and_apply_offline_skin', { accountId, playerName }),

  /** 移除离线账户皮肤（恢复默认） */
  removeOfflineSkin: (accountId: string) => invoke<void>('remove_offline_skin', { accountId }),

  /** 切换离线皮肤模型（variant：slim / classic；贴图不变） */
  setOfflineSkinVariant: (accountId: string, variant: string) =>
    invoke<void>('set_offline_skin_variant', { accountId, variant }),

  /** 应用内置预设皮肤（preset：steve / alex；模型随预设固定） */
  applyOfflinePreset: (accountId: string, preset: string) =>
    invoke<void>('apply_offline_preset', { accountId, preset }),

  /** 应用 URL 皮肤（仅 http/https；模型沿用当前设置） */
  applyOfflineSkinUrl: (accountId: string, url: string) =>
    invoke<void>('apply_offline_skin_url', { accountId, url }),

  /** 应用本地皮肤文件：path 为空时弹文件选择框（png） */
  applyOfflineSkinFile: (accountId: string, path?: string) =>
    invoke<void>('apply_offline_skin_file', { accountId, path: path ?? null }),

  /** 读取离线账户皮肤 PNG 的 data URL（3D 预览用；无皮肤返回 null） */
  getOfflineSkinDataUrl: (accountId: string) =>
    invoke<string | null>('get_offline_skin_data_url', { accountId }),

  /** 当前运行中的实例（内存/时长由前端轮询刷新） */
  listRunning: () => invoke<RunningState[]>('list_running'),

  /** 列出实例社区资源（模组 / 数据包 / 光影 / 资源包，resource_type 定目录） */
  listMods: (instanceId: string, resourceType: string) =>
    invoke<ModInfo[]>('list_mods', { instanceId, resourceType }),

  /** 启用 / 禁用模组（重命名 .jar ↔ .jar.disabled；仅模组支持） */
  toggleMod: (instanceId: string, resourceType: string, fileName: string, enable: boolean) =>
    invoke<void>('toggle_mod', { instanceId, resourceType, fileName, enable }),

  /** 删除资源到实例回收站（保留期 0 = 永久删除） */
  deleteMods: (instanceId: string, resourceType: string, fileNames: string[]) =>
    invoke<string[]>('delete_mods', { instanceId, resourceType, fileNames }),

  /** 实例回收站列表（实例级共用） */
  listTrash: (instanceId: string) => invoke<TrashItem[]>('list_trash', { instanceId }),

  /** 从回收站恢复到类型对应目录 */
  restoreMod: (instanceId: string, resourceType: string, trashName: string) =>
    invoke<void>('restore_mod', { instanceId, resourceType, trashName }),

  /** 清空回收站，返回清理文件数 */
  emptyTrash: (instanceId: string) => invoke<number>('empty_trash', { instanceId }),

  /** 复制外部文件到实例（拖放 / 从文件安装）；overwrite=false 时同名跳过 */
  installLocalMods: (instanceId: string, resourceType: string, paths: string[], overwrite: boolean) =>
    invoke<LocalInstallResult>('install_local_mods', {
      instanceId,
      resourceType,
      paths,
      overwrite,
    }),

  /** 在线搜索（origin = modrinth / curseforge；category 仅 Modrinth；CF 仅模组） */
  searchMods: (
    instanceId: string,
    resourceType: string,
    origin: string,
    query: string,
    category: string,
    sort: string,
    offset: number,
  ) =>
    invoke<{ hits: SearchHit[]; total: number }>('search_mods', {
      instanceId,
      resourceType,
      origin,
      query,
      category: category || null,
      sort,
      offset,
    }),

  /** 某资源的版本列表（按实例环境过滤，无兼容时回退全量） */
  listOnlineVersions: (instanceId: string, resourceType: string, origin: string, projectId: string) =>
    invoke<OnlineVersion[]>('list_online_versions', {
      instanceId,
      resourceType,
      origin,
      projectId,
    }),

  /** 生成安装计划（主文件 + 依赖树，已安装项被过滤） */
  planModInstall: (instanceId: string, resourceType: string, origin: string, versionId: string) =>
    invoke<InstallPlan>('plan_mod_install', { instanceId, resourceType, origin, versionId }),

  /** 确认安装（批量下载，sha1 校验；replaceFileNames 用于更新时移除旧文件） */
  confirmModInstall: (
    instanceId: string,
    resourceType: string,
    versionIds: string[],
    replaceFileNames?: string[],
  ) =>
    invoke<string[]>('confirm_mod_install', {
      instanceId,
      resourceType,
      versionIds,
      replaceFileNames: replaceFileNames ?? null,
    }),

  /** 检查在线来源资源的更新 */
  checkModUpdates: (instanceId: string, resourceType: string) =>
    invoke<UpdateInfo[]>('check_mod_updates', { instanceId, resourceType }),

  /** 模组图标代理（Rust 下载 + 磁盘缓存），返回 data URL */
  fetchIcon: (url: string) => invoke<string>('fetch_icon', { url }),

  /** 弹出整合包选择对话框并返回预览；取消返回 null */
  pickModpack: () => invoke<PackPreview | null>('pick_modpack'),

  /** 解析指定路径的整合包预览（拖入文件导入用） */
  parsePackFile: (path: string) => invoke<PackPreview>('parse_pack_file', { path }),

  /** 导入整合包（创建实例 + 安装加载器 + 下载 + 解压 + 补全资源），返回实例 ID */
  importModpack: (path: string, name: string) =>
    invoke<string>('import_modpack', { path, name }),

  /** 列出导出整合包的可选内容清单（按实例 minecraft/ 目录扫描） */
  listExportItems: (instanceId: string) =>
    invoke<ExportGroup[]>('list_export_items', { instanceId }),

  /** 导出当前实例为 .mrpack（保存对话框；selected 为勾选内容键，返回保存路径） */
  exportModpack: (instanceId: string, selected: string[]) =>
    invoke<string>('export_modpack', { instanceId, selected }),

  /**
   * 控制主窗口磨砂：优先 Win11 系统轻磨砂（模糊更弱），Win10 回退经典 Acrylic。
   * 返回实际通道：system / classic / off
   */
  setWindowBlur: (enabled: boolean, isDark: boolean, opacity: number) =>
    invoke<'system' | 'classic' | 'off'>('set_window_blur', {
      enabled,
      isDark,
      opacity,
    }),
}

/** 监听实例增删改事件（变更后应重拉列表） */
export function onInstancesChanged(cb: () => void): Promise<UnlistenFn> {
  return listen('instances-changed', () => cb())
}

/** 监听创建实例进度 */
export function onCreateProgress(cb: (p: CreateProgress) => void): Promise<UnlistenFn> {
  return listen<CreateProgress>('instance-create-progress', (e) => cb(e.payload))
}

/** 监听下载进度（Rust 引擎 200ms 节流聚合 + 文件完成即时推送） */
export function onDownloadProgress(cb: (p: DownloadProgress) => void): Promise<UnlistenFn> {
  return listen<DownloadProgress>('download-progress', (e) => cb(e.payload))
}

/** 监听 Java 安装完成（解压落位） */
export function onJavaInstalled(cb: (p: JavaInstalledPayload) => void): Promise<UnlistenFn> {
  return listen<JavaInstalledPayload>('java-installed', (e) => cb(e.payload))
}

/** 监听 Java 安装失败（解压阶段） */
export function onJavaInstallFailed(
  cb: (p: JavaInstallFailedPayload) => void,
): Promise<UnlistenFn> {
  return listen<JavaInstallFailedPayload>('java-install-failed', (e) => cb(e.payload))
}

/** 监听启动状态机阶段推进 */
export function onLaunchState(cb: (p: LaunchStatePayload) => void): Promise<UnlistenFn> {
  return listen<LaunchStatePayload>('launch-state', (e) => cb(e.payload))
}

/** 监听游戏进程逐行日志 */
export function onLaunchLog(cb: (p: LaunchLogPayload) => void): Promise<UnlistenFn> {
  return listen<LaunchLogPayload>('launch-log', (e) => cb(e.payload))
}

/** 监听游戏进程退出 */
export function onLaunchExited(cb: (p: LaunchExitedPayload) => void): Promise<UnlistenFn> {
  return listen<LaunchExitedPayload>('launch-exited', (e) => cb(e.payload))
}

/** 监听实例模组列表变更（启停 / 删除 / 拖放安装后应刷新） */
export function onModsChanged(cb: (instanceId: string) => void): Promise<UnlistenFn> {
  return listen<{ instanceId: string }>('mods-changed', (e) => cb(e.payload.instanceId))
}

/** 监听账户变更（添加 / 切换 / 删除后应刷新列表） */
export function onAccountsChanged(cb: () => void): Promise<UnlistenFn> {
  return listen('accounts-changed', () => cb())
}

/** 监听微软登录流程状态（设备码展示 / 认证中 / 成功 / 过期 / 拒绝 / 失败） */
export function onMsLoginState(cb: (p: MsLoginStatePayload) => void): Promise<UnlistenFn> {
  return listen<MsLoginStatePayload>('ms-login-state', (e) => cb(e.payload))
}

/** 监听 LittleSkin OAuth 设备码登录流程状态 */
export function onAuthlibOauthState(
  cb: (p: AuthlibOauthStatePayload) => void,
): Promise<UnlistenFn> {
  return listen<AuthlibOauthStatePayload>('authlib-oauth-state', (e) => cb(e.payload))
}

/** 监听整合包导入进度 */
export function onPackImportProgress(cb: (p: PackImportProgress) => void): Promise<UnlistenFn> {
  return listen<PackImportProgress>('pack-import-progress', (e) => cb(e.payload))
}
