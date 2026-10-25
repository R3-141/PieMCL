import type { ThemeMode } from '../types/ui'
import type { UiState } from '../stores/uiStore'
import { useBackgroundStore } from '../stores/backgroundStore'

const DARK_MEDIA = '(prefers-color-scheme: dark)'

/** 计算当前是否应呈现深色 */
export function resolveIsDark(mode: ThemeMode): boolean {
  return mode === 'dark' || (mode === 'system' && window.matchMedia(DARK_MEDIA).matches)
}

/**
 * 实际生效的界面明暗：自定义背景启用且用户显式选择了「深色 / 浅色」时覆盖主题设置
 * （让按钮 / 图标 / 文字与背景图协调）；auto 或未启用背景时跟随主题
 */
export function effectiveIsDark(mode: ThemeMode, hasBg: boolean, bgTheme: string): boolean {
  if (hasBg && (bgTheme === 'dark' || bgTheme === 'light')) return bgTheme === 'dark'
  return resolveIsDark(mode)
}

/** 切换根节点 .dark 类（深色变体全部由 CSS 负责） */
export function applyThemeClass(isDark: boolean): void {
  document.documentElement.classList.toggle('dark', isDark)
}

/** 注入自定义主色；悬停/软色由 CSS color-mix 自动派生，JS 不做色值计算 */
export function applyAccent(hex: string | null): void {
  const style = document.documentElement.style
  if (hex) style.setProperty('--accent', hex)
  else style.removeProperty('--accent')
}

/** 应用减少动画标志类 */
export function applyMotionFlags(reducedMotion: boolean): void {
  document.documentElement.classList.toggle('reduce-motion', reducedMotion)
}

type ThemeSlice = Pick<UiState, 'theme' | 'customAccent' | 'reducedMotion'>

/** 将 store 中的主题相关状态一次性同步到 DOM（幂等，可重复调用） */
export function syncThemeToDom(s: ThemeSlice): void {
  applyThemeClass(effectiveIsDark(s.theme, useBackgroundStore.getState().dataUrl !== null, useBackgroundStore.getState().theme))
  applyAccent(s.customAccent)
  applyMotionFlags(s.reducedMotion)
}

/** 跟随系统时的媒体监听（事件驱动，无轮询）；返回清理函数 */
export function watchSystemTheme(onChange: () => void): () => void {
  const mql = window.matchMedia(DARK_MEDIA)
  mql.addEventListener('change', onChange)
  return () => mql.removeEventListener('change', onChange)
}
