import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import './index.css'
import { useUiStore } from './stores/uiStore'
import { useBackgroundStore } from './stores/backgroundStore'
import { applyThemeClass, effectiveIsDark, syncThemeToDom, watchSystemTheme } from './lib/theme'
import type { ThemeMode } from './types/ui'

/**
 * 重算根节点 .dark 类：自定义背景启用且显式选择了界面明暗时覆盖主题设置
 * （syncThemeToDom 只按主题设置计算，覆盖须在其后调用）
 */
function refreshThemeClass(): void {
  const bg = useBackgroundStore.getState()
  const ui = useUiStore.getState()
  applyThemeClass(effectiveIsDark(ui.theme, bg.dataUrl !== null, bg.theme))
}

// 首次渲染前同步主题（persist 已从 localStorage 同步水合，避免首屏闪烁）
syncThemeToDom(useUiStore.getState())
refreshThemeClass()

// 仅当主题为“跟随系统”时挂媒体监听；离开后移除（事件驱动，无定时器）
let stopWatch: (() => void) | null = null
function syncSystemWatcher(mode: ThemeMode): void {
  if (mode === 'system') {
    stopWatch ??= watchSystemTheme(() => syncThemeToDom(useUiStore.getState()))
  } else {
    stopWatch?.()
    stopWatch = null
  }
}
syncSystemWatcher(useUiStore.getState().theme)

// 主题相关字段变化时同步到 DOM
useUiStore.subscribe((s, prev) => {
  if (
    s.theme !== prev.theme ||
    s.customAccent !== prev.customAccent ||
    s.reducedMotion !== prev.reducedMotion
  ) {
    syncThemeToDom(s)
    refreshThemeClass()
    if (s.theme !== prev.theme) syncSystemWatcher(s.theme)
  }
})

// 背景图启用 / 界面明暗覆盖变化时重算根节点明暗
useBackgroundStore.subscribe((s, prev) => {
  if (s.dataUrl !== prev.dataUrl || s.theme !== prev.theme) refreshThemeClass()
})

// 系统主题变化（跟随系统模式）也需考虑背景覆盖
watchSystemTheme(() => refreshThemeClass())

// 禁用 WebView 默认右键菜单（避免浏览器菜单破坏桌面应用观感）；输入框保留以便粘贴
document.addEventListener('contextmenu', (e) => {
  const t = e.target as HTMLElement | null
  if (!t || (t.tagName !== 'INPUT' && t.tagName !== 'TEXTAREA' && !t.isContentEditable)) {
    e.preventDefault()
  }
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
