import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { ThemeMode, ViewId } from '../types/ui'
import { ALL_VIEW_IDS } from '../lib/nav'

/** 仅存放 UI 状态；业务数据一律由 Rust 侧管理 */
export interface UiState {
  currentView: ViewId
  theme: ThemeMode
  /** 自定义主色（hex，如 "#4f7cff"）；null 表示使用默认主色 */
  customAccent: string | null
  sidebarCollapsed: boolean
  /** 「社区资源」父级分组展开状态（默认收起） */
  communityExpanded: boolean
  /** 下载面板可见性（瞬态，不持久化） */
  downloadPanelVisible: boolean
  /** 下载面板可见性（瞬态，不持久化） */
  reducedMotion: boolean
  /** 隐藏的平级导航入口（社区资源子项不参与隐藏管理） */
  hiddenNavItems: ViewId[]
  /** 全窗拖放分流：resource = 安装社区资源（默认）；skin = 皮肤上传弹窗接管 */
  dragMode: 'resource' | 'skin'
  /** 实例页当前选中的实例（瞬态，不持久化） */
  selectedInstanceId: string | null
  /** 待打开的账户详情（瞬态：首页三个点 → 账户管理详情页接力，PlayerView 挂载时消费） */
  pendingAccountDetail: string | null
  /** 拖入的整合包路径（瞬态：实例页拖入 → 导入对话框接力，InstancesView 消费） */
  packDropPath: string | null
  /** 皮肤拖放处理函数（皮肤上传弹窗打开时注册） */
  skinDropHandler: ((pngPath: string) => void) | null
  /** 标题栏与侧栏的系统磨砂质感（Acrylic）；内容区不受影响 */
  blurEnabled: boolean
  /** 框架叠加色的基础透明度 0–100（标题栏 / 侧栏 / 抽屉 / 内容区按固定层次派生） */
  blurStrength: number
  /** 框架叠加色（hex）；null = 跟随主题（浅色白 / 深色黑） */
  blurTint: string | null
  /** 自定义背景下的卡片不透明度 0–100（100 = 不透明） */
  cardOpacity: number
  /** 自定义背景下的卡片磨砂（backdrop blur） */
  cardBlur: boolean
  /** 自定义背景下的卡片磨砂色（hex）；null = 跟随主题（浅色白 / 深色黑） */
  cardTint: string | null

  setCurrentView: (v: ViewId) => void
  setTheme: (t: ThemeMode) => void
  setCustomAccent: (hex: string | null) => void
  toggleSidebar: () => void
  setCommunityExpanded: (v: boolean) => void
  setDownloadPanelVisible: (v: boolean) => void
  setReducedMotion: (v: boolean) => void
  setHiddenNavItems: (items: ViewId[]) => void
  toggleNavItemHidden: (id: ViewId) => void
  setDragMode: (m: 'resource' | 'skin') => void
  setSkinDropHandler: (fn: ((pngPath: string) => void) | null) => void
  setSelectedInstance: (id: string | null) => void
  setPendingAccountDetail: (id: string | null) => void
  setPackDropPath: (path: string | null) => void
  setBlurEnabled: (v: boolean) => void
  setBlurStrength: (v: number) => void
  setBlurTint: (hex: string | null) => void
  setCardOpacity: (v: number) => void
  setCardBlur: (v: boolean) => void
  setCardTint: (hex: string | null) => void
}

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      currentView: 'home',
      theme: 'system',
      customAccent: null,
      sidebarCollapsed: false,
      communityExpanded: false,
      downloadPanelVisible: false,
      reducedMotion: false,
      hiddenNavItems: [],
      dragMode: 'resource',
      selectedInstanceId: null,
      pendingAccountDetail: null,
      packDropPath: null,
      skinDropHandler: null,
      blurEnabled: true,
      blurStrength: 70,
      blurTint: null,
      cardOpacity: 50,
      cardBlur: true,
      cardTint: null,

      setCurrentView: (v) => set({ currentView: v }),
      setTheme: (t) => set({ theme: t }),
      setCustomAccent: (hex) => set({ customAccent: hex }),
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      setCommunityExpanded: (v) => set({ communityExpanded: v }),
      setDownloadPanelVisible: (v) => set({ downloadPanelVisible: v }),
      setReducedMotion: (v) => set({ reducedMotion: v }),
      setHiddenNavItems: (items) => set({ hiddenNavItems: items }),
      toggleNavItemHidden: (id) =>
        set((s) => {
          const hiddenNavItems = s.hiddenNavItems.includes(id)
            ? s.hiddenNavItems.filter((v) => v !== id)
            : [...s.hiddenNavItems, id]
          // 至少保留一个可见入口
          if (hiddenNavItems.length >= ALL_VIEW_IDS.length) return s
          // 当前视图被隐藏时，跳到第一个仍可见的入口
          const currentView = hiddenNavItems.includes(s.currentView)
            ? (ALL_VIEW_IDS.find((v) => !hiddenNavItems.includes(v)) ?? 'settings')
            : s.currentView
          return { hiddenNavItems, currentView }
        }),
      setDragMode: (m) => set({ dragMode: m }),
      setSkinDropHandler: (fn) => set({ skinDropHandler: fn }),
      setSelectedInstance: (id) => set({ selectedInstanceId: id }),
      setPendingAccountDetail: (id) => set({ pendingAccountDetail: id }),
      setPackDropPath: (path) => set({ packDropPath: path }),
      setBlurEnabled: (v) => set({ blurEnabled: v }),
      setBlurStrength: (v) => set({ blurStrength: v }),
      setBlurTint: (hex) => set({ blurTint: hex }),
      setCardOpacity: (v) => set({ cardOpacity: v }),
      setCardBlur: (v) => set({ cardBlur: v }),
      setCardTint: (hex) => set({ cardTint: hex }),
    }),
    {
      name: 'piemcl-ui',
      // 只持久化用户偏好；downloadPanelVisible 为瞬态，排除
      partialize: (s) => ({
        currentView: s.currentView,
        theme: s.theme,
        customAccent: s.customAccent,
        sidebarCollapsed: s.sidebarCollapsed,
        communityExpanded: s.communityExpanded,
        reducedMotion: s.reducedMotion,
        hiddenNavItems: s.hiddenNavItems,
        blurEnabled: s.blurEnabled,
        blurStrength: s.blurStrength,
        blurTint: s.blurTint,
        cardOpacity: s.cardOpacity,
        cardBlur: s.cardBlur,
        cardTint: s.cardTint,
      }),
      // 数据修复（merge 在 rehydrate 同步执行，早于任何组件读取）：
      // 设置入口永不可隐藏；已移除视图（旧社区子页 / 下载页）的残留清洗；
      // 非法 currentView 回落实例页
      merge: (persisted, current) => {
        const p = (persisted ?? {}) as Partial<UiState>
        const hiddenNavItems = (p.hiddenNavItems ?? current.hiddenNavItems).filter(
          (v) => v !== 'settings' && v !== 'home',
        )
        // 首页为启动器默认页：每次启动强制进入首页（会话内切换正常记忆）
        return { ...current, ...p, hiddenNavItems, currentView: 'home' }
      },
    },
  ),
)
