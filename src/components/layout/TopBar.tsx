import { getCurrentWindow } from '@tauri-apps/api/window'
import { useJavaStore } from '../../stores/javaStore'
import { useDownloadStore } from '../../stores/downloadStore'
import { useUiStore } from '../../stores/uiStore'
import { useBackgroundStore } from '../../stores/backgroundStore'
import { formatSpeed } from '../../lib/format'
import {
  IconClose,
  IconDownloads,
  IconJava,
  IconMinus,
} from '../common/Icons'

/** 窗口控制（最小化 / 关闭） */
function WindowControls() {
  const appWindow = getCurrentWindow()
  const btn =
    'flex w-11 shrink-0 items-center justify-center text-neutral-500 transition-colors duration-150 dark:text-neutral-400'
  return (
    <div className="flex items-stretch self-stretch">
      <button type="button" title="最小化" onClick={() => appWindow.minimize()} className={`${btn} hover:bg-neutral-200/70 dark:hover:bg-neutral-700/70`}>
        <IconMinus className="size-4" />
      </button>
      <button type="button" title="关闭" onClick={() => appWindow.close()} className={`${btn} hover:bg-red-500 hover:text-white`}>
        <IconClose className="size-4" />
      </button>
    </div>
  )
}

/**
 * 顶部标题栏（一体化窗口）：整条可拖动移动窗口；
 * 右端为窗口控制（最小化 / 关闭）。
 * 磨砂开启时半透明（比侧栏更淡），透出系统 Acrylic 模糊背景
 */
export function TopBar() {
  const downloadPanelVisible = useUiStore((s) => s.downloadPanelVisible)
  const toggleDownloadPanel = useUiStore((s) => s.setDownloadPanelVisible)
  const setCurrentView = useUiStore((s) => s.setCurrentView)
  const tasks = useDownloadStore((s) => s.tasks)
  const javas = useJavaStore((s) => s.javas)
  const javaLoading = useJavaStore((s) => s.loading)
  const blurEnabled = useUiStore((s) => s.blurEnabled)
  // 自定义背景启用时同样使用半透明标题栏层（背景图在窗口最底层透出）
  const glass = blurEnabled || useBackgroundStore((s) => s.dataUrl !== null)

  // 下载活动摘要：运行/暂停任务数与总速度
  const taskList = Object.values(tasks)
  const active = taskList.filter((t) => t.state === 'running' || t.state === 'paused')
  const failed = taskList.filter((t) => t.state === 'failed')
  const totalSpeed = active.reduce((sum, t) => sum + t.bytesPerSecond, 0)

  return (
    <header
      data-tauri-drag-region
      className={`flex h-12 shrink-0 items-center gap-3 px-4 ${
        // 标题栏整条最深（含侧栏品牌区），与下方区域形成视觉切割；
        // 半透明层开启时边框弱化，避免浅色底上出现亮白条
        glass
          ? 'frame-title border-b border-black/10'
          : 'bg-white border-b border-neutral-200 dark:bg-neutral-900 dark:border-neutral-800'
      }`}
    >
      {/* 拖动弹性区（data-tauri-drag-region：空白处按住即可移动窗口） */}
      <div data-tauri-drag-region className="h-full flex-1" />

      {/* 下载活动：点击切换面板（再点一次收回） */}
      <button
        type="button"
        onClick={() => toggleDownloadPanel(!downloadPanelVisible)}
        className={`flex items-center gap-2 rounded-lg px-2.5 py-1 text-xs transition-colors duration-150 ${
          failed.length > 0
            ? 'bg-red-50 text-red-600 hover:bg-red-100 dark:bg-red-500/10 dark:text-red-300'
            : downloadPanelVisible
              ? 'bg-accent-soft text-accent'
              : 'bg-neutral-100 text-neutral-500 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-400 dark:hover:bg-neutral-700'
        }`}
      >
        <IconDownloads className="size-4" />
        {active.length > 0
          ? `下载中 ×${active.length} · ${formatSpeed(totalSpeed)}`
          : failed.length > 0
            ? `${failed.length} 个任务失败`
            : '下载队列'}
      </button>

      {/* Java 状态：点击进入 Java 管理页 */}
      <button
        type="button"
        onClick={() => setCurrentView('java')}
        title="Java 管理"
        className="flex items-center gap-2 rounded-lg bg-neutral-100 px-2.5 py-1 text-xs text-neutral-500 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-400 dark:hover:bg-neutral-700"
      >
        <IconJava className="size-4" />
        {javaLoading
          ? 'Java：检测中…'
          : javas === null
            ? 'Java：未检测'
            : javas.length > 0
              ? `Java ×${javas.length}`
              : 'Java：未检测到'}
      </button>

      <WindowControls />
    </header>
  )
}
