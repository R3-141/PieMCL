import { create } from 'zustand'

/**
 * 资源相关页面的跨组件 UI 状态（轻量）：
 * - marketInstanceId：「模组市场」页选中的安装目标实例（页面内实例选择器驱动）
 * - marketTab：模组市场页当前的类型 Tab（模组 / 数据包 / 光影包 / 资源包），
 *   市场页拖放按此 Tab 类型落位
 * - resourceInstanceId：「资源管理」页（实例附属页，从实例卡片「更多」进入）
 *   绑定的实例；资源管理页拖放按 resourceTab 落位到该实例
 * - dragActive：文件拖入窗口时的全窗覆盖层开关
 * - dropMessage：最近一次拖放安装的结果提示（页面顶部展示）
 * 资源列表数据放各页面本地 state（页面级数据，切实例即重取）
 */
import type { ResourceKind } from '../views/community/config'

/** 资源管理页的 Tab 类型：四类资源 + 截图 + 存档（后两者为媒体管理，非资源落位） */
export type ResourceTab = ResourceKind | 'screenshots' | 'saves'

export interface ModsUiState {
  marketInstanceId: string | null
  marketTab: ResourceKind
  resourceInstanceId: string | null
  resourceTab: ResourceTab
  dragActive: boolean
  dropMessage: string | null

  setMarketInstance: (id: string | null) => void
  setMarketTab: (t: ResourceKind) => void
  setResourceInstance: (id: string | null) => void
  setResourceTab: (t: ResourceTab) => void
  setDragActive: (v: boolean) => void
  setDropMessage: (msg: string | null) => void
}

export const useModsStore = create<ModsUiState>()((set) => ({
  marketInstanceId: null,
  marketTab: 'mods',
  resourceInstanceId: null,
  resourceTab: 'mods',
  dragActive: false,
  dropMessage: null,

  setMarketInstance: (id) => set({ marketInstanceId: id }),
  setMarketTab: (t) => set({ marketTab: t }),
  setResourceInstance: (id) => set({ resourceInstanceId: id }),
  setResourceTab: (t) => set({ resourceTab: t }),
  setDragActive: (v) => set({ dragActive: v }),
  setDropMessage: (msg) => set({ dropMessage: msg }),
}))
