import NAV_LOGO from '../../assets/logo.png'
import { NAV_BOTTOM, NAV_TOP } from '../../lib/nav'
import { useUiStore } from '../../stores/uiStore'
import { useBackgroundStore } from '../../stores/backgroundStore'
import { IconChevrons } from '../common/Icons'

interface ItemRenderProps {
  icon: React.ComponentType<{ className?: string }>
  label: string
  active: boolean
  collapsed: boolean
  onClick: () => void
}

/** 单个导航按钮 */
function NavButton({ icon: Icon, label, active, collapsed, onClick }: ItemRenderProps) {
  return (
    <button
      type="button"
      title={collapsed ? label : undefined}
      onClick={onClick}
      className={`flex w-full items-center rounded-lg text-sm transition-colors duration-150 ${
        active
          ? 'bg-accent-soft font-medium text-accent'
          : 'text-neutral-600 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800'
      } ${collapsed ? 'justify-center px-0 py-2' : 'gap-3 px-3 py-2'}`}
    >
      <Icon className="size-5 shrink-0" />
      {!collapsed && <span className="truncate">{label}</span>}
    </button>
  )
}

/** 左侧可折叠导航栏（磨砂开启时半透明，透出系统 Acrylic 模糊背景）。
 * 实例附属页（资源管理）打开时高亮「实例」项。 */
export function Sidebar() {
  const currentView = useUiStore((s) => s.currentView)
  const collapsed = useUiStore((s) => s.sidebarCollapsed)
  const hiddenNavItems = useUiStore((s) => s.hiddenNavItems)
  const setCurrentView = useUiStore((s) => s.setCurrentView)
  const toggleSidebar = useUiStore((s) => s.toggleSidebar)
  const blurEnabled = useUiStore((s) => s.blurEnabled)
  // 自定义背景启用时同样使用半透明框架层（背景图在窗口最底层透出）
  const glass = blurEnabled || useBackgroundStore((s) => s.dataUrl !== null)

  // 设置入口永不可隐藏（防御：即使持久化数据中残留也强制显示）
  const topItems = NAV_TOP.filter((it) => !hiddenNavItems.includes(it.id))
  const bottomItems = NAV_BOTTOM.filter(
    (it) => it.id === 'settings' || !hiddenNavItems.includes(it.id),
  )
  // 资源管理页为实例附属页：侧栏高亮「实例」
  const highlightView = currentView === 'resource' ? 'instances' : currentView

  return (
    <aside
      className={`relative z-10 flex shrink-0 flex-col transition-[width] duration-200 ${
        // 半透明层开启时 aside 本体透明：品牌区与导航区各自单层着色，避免双层叠加变黑
        glass
          ? 'border-r border-black/10'
          : 'bg-white border-r border-neutral-200 dark:bg-neutral-900 dark:border-neutral-800'
      } ${collapsed ? 'w-16' : 'w-52'}`}
    >
      {/* 品牌区：与顶部标题栏同层同深，连成一条完整的标题横条做视觉切割 */}
      <div
        className={`flex h-12 items-center gap-2.5 border-b px-4 ${
          glass ? 'frame-title border-black/10' : 'border-neutral-200 dark:border-neutral-800'
        }`}
      >
        <img
          src={NAV_LOGO}
          alt="PieMCL"
          className="size-7 shrink-0 rounded-lg"
          draggable={false}
        />
        {!collapsed && <span className="truncate text-base font-bold tracking-wide">PieMCL</span>}
      </div>

      {/* 导航区 + 折叠开关：半透明中间层 */}
      <div className={`flex min-h-0 flex-1 flex-col ${glass ? 'frame-side' : ''}`}>
        <nav className="flex-1 space-y-1 overflow-y-auto p-2">
          {topItems.map((it) => (
            <NavButton
              key={it.id}
              icon={it.icon}
              label={it.label}
              active={it.id === highlightView}
              collapsed={collapsed}
              onClick={() => setCurrentView(it.id)}
            />
          ))}
          {bottomItems.map((it) => (
            <NavButton
              key={it.id}
              icon={it.icon}
              label={it.label}
              active={it.id === highlightView}
              collapsed={collapsed}
              onClick={() => setCurrentView(it.id)}
            />
          ))}
        </nav>

        {/* 折叠开关 */}
        <div
          className={`border-t p-2 ${
            glass ? 'border-black/10' : 'border-neutral-200 dark:border-neutral-800'
          }`}
        >
          <button
            type="button"
            title={collapsed ? '展开侧栏' : '折叠侧栏'}
            onClick={toggleSidebar}
            className="flex w-full items-center justify-center rounded-lg py-2 text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
          >
            <IconChevrons
              className={`size-5 transition-transform duration-200 ${collapsed ? 'rotate-180' : ''}`}
            />
          </button>
        </div>
      </div>
    </aside>
  )
}
