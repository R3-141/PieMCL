import type { ComponentType } from 'react'
import type { ViewId } from '../types/ui'
import {
  IconConfig,
  IconHome,
  IconInstances,
  IconJava,
  IconLan,
  IconLogs,
  IconMods,
  IconPlayer,
  IconServer,
  IconSettings,
} from '../components/common/Icons'

export interface NavItem {
  id: ViewId
  label: string
  icon: ComponentType<{ className?: string }>
}

/** 可折叠的父级分组（当前无分组，结构保留供后续扩展） */
export interface NavGroup {
  id: string
  label: string
  icon: ComponentType<{ className?: string }>
  items: NavItem[]
}

/** 分组前的平级入口 */
export const NAV_TOP: readonly NavItem[] = [
  { id: 'home', label: '首页', icon: IconHome },
  { id: 'instances', label: '实例', icon: IconInstances },
  { id: 'market', label: '模组市场', icon: IconMods },
  { id: 'servers', label: '服务器', icon: IconServer },
  { id: 'lan', label: '联机', icon: IconLan },
]

/** 分组后的平级入口 */
export const NAV_BOTTOM: readonly NavItem[] = [
  { id: 'gameconfig', label: '游戏配置', icon: IconConfig },
  { id: 'java', label: 'Java', icon: IconJava },
  { id: 'log', label: '日志', icon: IconLogs },
  { id: 'player', label: '账户管理', icon: IconPlayer },
  { id: 'settings', label: '设置', icon: IconSettings },
]

/** 平级入口合集（设置页「导航入口显隐」用；附属页不参与隐藏管理） */
export const NAV_MAIN: readonly NavItem[] = [...NAV_TOP, ...NAV_BOTTOM]

/** 所有视图 id（uiStore 合法性校验 / 兜底用；含实例附属页 resource） */
export const ALL_VIEW_IDS: readonly ViewId[] = [
  ...NAV_TOP.map((i) => i.id),
  'resource',
  ...NAV_BOTTOM.map((i) => i.id),
]
