/** 主题模式：浅色 / 深色 / 跟随系统 */
export type ThemeMode = 'light' | 'dark' | 'system'

/** 主导航视图标识（即导航入口 id）。
 * `home` = 首页（启动器默认页：账户卡 + 右下角启动按钮）；
 * `market` = 模组市场（侧栏入口，页内选类型与安装目标实例）；
 * `resource` = 实例附属页（从实例「更多 → 资源管理」进入，不渲染在侧栏），
 * 绑定单个实例管理其模组 / 数据包 / 光影包 / 资源包。
 * 注：下载进度统一走顶栏的下载面板，无独立「下载」页。 */
export type ViewId =
  | 'home'
  | 'instances'
  | 'market'
  | 'servers'
  | 'lan'
  | 'resource'
  | 'gameconfig'
  | 'java'
  | 'log'
  | 'player'
  | 'settings'
