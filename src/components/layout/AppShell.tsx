import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { ViewId } from '../../types/ui'
import { useUiStore } from '../../stores/uiStore'
import { useDownloadStore } from '../../stores/downloadStore'
import { useJavaStore } from '../../stores/javaStore'
import { useLaunchStore, isBusy } from '../../stores/launchStore'
import { useModsStore } from '../../stores/modsStore'
import { useBackgroundStore } from '../../stores/backgroundStore'
import { ipc, onDownloadProgress, onJavaInstallFailed, onJavaInstalled, onLaunchExited, onLaunchLog, onLaunchState } from '../../lib/ipc'
import { hexToRgbTriple } from '../../lib/color'
import { effectiveIsDark, watchSystemTheme } from '../../lib/theme'
import { HomeView } from '../../views/home/HomeView'
import { InstancesView } from '../../views/instances/InstancesView'
import { ResourceView } from '../../views/resource/ResourceView'
import { MarketView } from '../../views/market/MarketView'
import { RESOURCE_CONFIGS, type ResourceConfig, type ResourceKind } from '../../views/community/config'
import { JavaView } from '../../views/java/JavaView'
import { LogView } from '../../views/log/LogView'
import { PlayerView } from '../../views/player/PlayerView'
import { SettingsView } from '../../views/settings/SettingsView'
import { GameConfigView } from '../../views/gameconfig/GameConfigView'
import { ServersView } from '../../views/servers/ServersView'
import { LanView } from '../../views/lan/LanView'
import { Sidebar } from './Sidebar'
import { TopBar } from './TopBar'
import { DownloadPanel } from './DownloadPanel'

const VIEWS: Record<ViewId, () => React.JSX.Element> = {
  home: HomeView,
  instances: InstancesView,
  market: () => <MarketView />,
  resource: () => <ResourceView instanceId={useModsStore.getState().resourceInstanceId ?? ''} onBack={() => useUiStore.getState().setCurrentView('instances')} />,
  gameconfig: () => <GameConfigView />,
  servers: () => <ServersView />,
  lan: () => <LanView />,
  java: JavaView,
  log: LogView,
  player: PlayerView,
  settings: SettingsView,
}

/** 一体化窗口：无边框 + 自绘标题栏，内容贴边铺满（无透明环带与圆角） */
export function AppShell() {
  const currentView = useUiStore((s) => s.currentView)
  // 兜底：持久化的 currentView 若指向已移除的视图（merge 清洗失效的极端情况），回落实例页
  const View = VIEWS[currentView] ?? InstancesView

  // 全局事件接线：下载进度 → downloadStore（零轮询，事件驱动）
  useEffect(() => {
    let unlisten: (() => void) | undefined
    onDownloadProgress((p) => useDownloadStore.getState().applyProgress(p)).then(
      (u) => (unlisten = u),
    )
    return () => unlisten?.()
  }, [])

  // Java 安装完成 → 刷新检测列表；失败 → 以失败任务形式进下载面板
  useEffect(() => {
    const offs: Array<() => void> = []
    onJavaInstalled((p) => {
      useJavaStore.getState().refresh()
      useDownloadStore.getState().applyProgress({
        taskId: `java-install-${p.major}-${Date.now()}`,
        kind: 'java',
        instanceId: `java:${p.major}`,
        state: 'completed',
        currentFile: p.path,
        completedFiles: 1,
        totalFiles: 1,
        downloadedBytes: 0,
        totalBytes: 0,
        bytesPerSecond: 0,
        error: null,
      })
    }).then((u) => offs.push(u))
    onJavaInstallFailed((p) => {
      useDownloadStore.getState().applyProgress({
        taskId: `java-install-${p.major}-${Date.now()}`,
        kind: 'java',
        instanceId: `java:${p.major}`,
        state: 'failed',
        currentFile: null,
        completedFiles: 0,
        totalFiles: 1,
        downloadedBytes: 0,
        totalBytes: 0,
        bytesPerSecond: 0,
        error: p.error,
      })
    }).then((u) => offs.push(u))
    return () => offs.forEach((u) => u())
  }, [])

  // 启动流程事件接线：状态机 / 逐行日志 / 进程退出
  useEffect(() => {
    const offs: Array<() => void> = []
    onLaunchState((p) => {
      useLaunchStore.getState().applyState(p)
      // 启动流程推进时切换到「日志」页（running 不打断用户当前操作）
      if (p.stage !== 'running') {
        useUiStore.getState().setCurrentView('log')
      }
    }).then((u) => offs.push(u))
    onLaunchLog((p) => useLaunchStore.getState().appendLog(p.instanceId, p.stream, p.line)).then(
      (u) => offs.push(u),
    )
    onLaunchExited((p) => useLaunchStore.getState().applyExit(p.instanceId, p.code, p.summary)).then(
      (u) => offs.push(u),
    )
    return () => offs.forEach((u) => u())
  }, [])

  // 启动器退出保护：有游戏运行时询问处理方式
  useEffect(() => {
    let unlisten: (() => void) | undefined
    getCurrentWindow()
      .onCloseRequested(async (event) => {
        const running = Object.values(useLaunchStore.getState().entries).filter(
          (e) => isBusy(e),
        )
        if (running.length === 0) return
        event.preventDefault()
        const keep = window.confirm(
          `有 ${running.length} 个游戏正在运行。\n\n` +
            '「确定」= 保留游戏运行并退出启动器\n「取消」= 结束游戏后退出启动器',
        )
        if (keep) {
          await getCurrentWindow().destroy()
        } else {
          for (const e of running) {
            await ipc.killInstance(e.instanceId).catch(() => {})
          }
          await getCurrentWindow().destroy()
        }
      })
      .then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [])

  // 启动时检测一次系统 Java（TopBar / Java 页 / 编辑对话框共享）
  useEffect(() => {
    useJavaStore.getState().refresh()
  }, [])

  // 启动时加载自定义背景（设置启用且文件存在才生效）
  useEffect(() => {
    void useBackgroundStore.getState().load()
  }, [])

  // 全窗拖放分流（优先级从高到低）：
  // 1. skin 模式：账户详情页的皮肤上传弹窗打开时，拖入的 PNG 交给弹窗处理
  // 2. 资源管理页：按当前 Tab 类型 + 绑定实例落位（.jar/.zip 由类型决定）
  // 3. 其他页面：提示引导到「更多 → 资源管理」
  useEffect(() => {
    const installTo = async (
      paths: string[],
      overwrite: boolean,
      cfg: ResourceConfig,
      instanceId: string,
    ): Promise<void> => {
      const accepted = paths.filter((p) => p.toLowerCase().endsWith(cfg.acceptExt))
      const rejected = paths.length - accepted.length
      if (accepted.length === 0) {
        window.alert(`仅支持 ${cfg.acceptExt} 文件（${cfg.title}）`)
        return
      }
      try {
        const r = await ipc.installLocalMods(instanceId, cfg.resourceType, accepted, overwrite)
        if (r.skipped.length > 0 && !overwrite) {
          const ok = window.confirm(
            `以下文件已存在：\n\n${r.skipped.join('\n')}\n\n是否覆盖安装？`,
          )
          if (ok) {
            await installTo(accepted, true, cfg, instanceId)
            return
          }
        }
        const parts: string[] = []
        if (r.installed.length > 0) parts.push(`已安装 ${r.installed.length} 个${cfg.title}`)
        if (r.skipped.length > 0) parts.push(`跳过已存在 ${r.skipped.length} 个`)
        if (rejected > 0) parts.push(`忽略非 ${cfg.acceptExt} 文件 ${rejected} 个`)
        if (r.failed.length > 0) parts.push(`失败 ${r.failed.length} 个`)
        useModsStore
          .getState()
          .setDropMessage(parts.join('，') || `没有可安装的 ${cfg.acceptExt} 文件`)
      } catch (e) {
        window.alert(`${cfg.title}安装失败：${e}`)
      }
    }
    // 允许拖放导入的视图：资源管理（按 Tab 分流）、实例（整合包导入）；
    // 皮肤上传弹窗打开时全局接管。其余页面（首页/市场/设置等）完全禁用拖放。
    const dropAllowed = (mode: string, view: string) =>
      mode === 'skin' || view === 'resource' || view === 'instances'
    let unlisten: (() => void) | undefined
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const s = useModsStore.getState()
        if (event.payload.type === 'enter' || event.payload.type === 'over') {
          const ui = useUiStore.getState()
          if (dropAllowed(ui.dragMode, ui.currentView)) {
            if (!s.dragActive) s.setDragActive(true)
          } else if (s.dragActive) {
            s.setDragActive(false)
          }
        } else if (event.payload.type === 'leave') {
          s.setDragActive(false)
        } else if (event.payload.type === 'drop') {
          s.setDragActive(false)
          const paths = 'paths' in event.payload ? (event.payload.paths as string[]) : []
          if (paths.length === 0) return
          const ui = useUiStore.getState()
          // 1. 皮肤上传弹窗接管
          if (ui.dragMode === 'skin' && ui.skinDropHandler) {
            const png = paths.find((p) => p.toLowerCase().endsWith('.png'))
            if (png) {
              ui.skinDropHandler(png)
            } else {
              window.alert('请拖入 PNG 格式的皮肤文件')
            }
            return
          }
          // 2. 资源管理页：按当前 Tab 类型 + 绑定实例落位（截图/存档 Tab 不接受资源拖放）
          if (ui.currentView === 'resource') {
            const ms = useModsStore.getState()
            const cfg = RESOURCE_CONFIGS[ms.resourceTab as ResourceKind]
            if (!cfg) return
            const instanceId = ms.resourceInstanceId
            if (!instanceId) {
              window.alert('请先从实例的「更多 → 资源管理」进入后再拖放安装')
              return
            }
            void installTo(paths, false, cfg, instanceId)
            return
          }
          // 3. 实例页：识别整合包（.mrpack / .zip）→ 打开导入对话框
          if (ui.currentView === 'instances') {
            const pack = paths.find((p) => {
              const ext = p.slice(p.lastIndexOf('.') + 1).toLowerCase()
              return ext === 'mrpack' || ext === 'zip'
            })
            if (pack) {
              useUiStore.getState().setPackDropPath(pack)
              return
            }
            window.alert(
              '拖入的文件不是整合包（仅支持 .mrpack / .zip）；\n安装资源请到对应实例的「资源管理」页拖入',
            )
            return
          }
          // 4. 其余页面：拖放已禁用，静默忽略
        }
      })
      .then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [])

  // 磨砂质感开关 → 系统 Acrylic 效果（模糊强度由 Windows 固定）
  const blurEnabled = useUiStore((s) => s.blurEnabled)
  const theme = useUiStore((s) => s.theme)
  // 自定义背景：启用时关闭系统磨砂（互斥），框架层改用半透明色罩叠在背景图上
  const bgDataUrl = useBackgroundStore((s) => s.dataUrl)
  const bgBlur = useBackgroundStore((s) => s.blur)
  const bgDim = useBackgroundStore((s) => s.dim)
  const hasBg = bgDataUrl !== null
  // 拖放模式（skin = 皮肤上传弹窗接管覆盖层文案与 drop 分流）
  const dragMode = useUiStore((s) => s.dragMode)
  // 资源管理页当前类型 Tab（覆盖层文案按其类型显示）
  const resourceTab = useModsStore((s) => s.resourceTab)
  // 生效主题（浅/深）：磨砂底色跟随应用主题；背景启用且显式选择明暗时以覆盖为准
  const bgTheme = useBackgroundStore((s) => s.theme)
  const [isDark, setIsDark] = useState(() =>
    effectiveIsDark(useUiStore.getState().theme, bgDataUrl !== null, bgTheme),
  )
  useEffect(() => {
    setIsDark(effectiveIsDark(theme, bgDataUrl !== null, bgTheme))
  }, [theme, bgDataUrl, bgTheme])
  useEffect(
    () => watchSystemTheme(() => setIsDark(effectiveIsDark(useUiStore.getState().theme, useBackgroundStore.getState().dataUrl !== null, useBackgroundStore.getState().theme))),
    [],
  )

  useEffect(() => {
    const apply = async () => {
      try {
        // 自定义背景启用时关闭系统磨砂（窗口实色，由 WebView 内背景图接管）；
        // 其余情况：优先系统轻磨砂（模糊弱），Win10 回退经典 Acrylic；
        // 经典通道的着色透明度固定 160/255，保持通透
        await ipc.setWindowBlur(blurEnabled && !hasBg, isDark, 160)
      } catch (e) {
        // 系统不支持时降级为实色外观；打印便于诊断
        console.error('应用窗口材质失败：', e)
      }
    }
    apply()
  }, [blurEnabled, hasBg, isDark])

  // 磨砂浓度与色调 → CSS 变量（frame-* 分层类消费）
  const blurStrength = useUiStore((s) => s.blurStrength)
  const blurTint = useUiStore((s) => s.blurTint)
  // 背景模式下的卡片样式（不透明度 / 磨砂 / 磨砂色）
  const cardOpacity = useUiStore((s) => s.cardOpacity)
  const cardBlur = useUiStore((s) => s.cardBlur)
  const cardTint = useUiStore((s) => s.cardTint)
  // 模组拖放覆盖层
  const dragActive = useModsStore((s) => s.dragActive)
  useEffect(() => {
    document.documentElement.style.setProperty('--blur-strength', String(blurStrength / 100))
  }, [blurStrength])
  useEffect(() => {
    const triple = blurTint ? hexToRgbTriple(blurTint) : null
    if (triple) {
      document.documentElement.style.setProperty('--frame-tint', triple)
    } else {
      document.documentElement.style.removeProperty('--frame-tint') // 回落主题默认（浅白/深黑）
    }
  }, [blurTint])

  // 背景模式卡片不透明度 → CSS 变量（.has-bg 覆盖规则消费）
  useEffect(() => {
    document.documentElement.style.setProperty('--card-alpha', String(cardOpacity / 100))
  }, [cardOpacity])

  // 卡片磨砂色：自定义时浅深两套变量同色；null 回落主题默认（浅白 / 深黑）
  useEffect(() => {
    const style = document.documentElement.style
    if (cardTint) {
      const triple = hexToRgbTriple(cardTint)
      style.setProperty('--card-tint', triple)
      style.setProperty('--card-tint-dark', triple)
    } else {
      style.removeProperty('--card-tint')
      style.removeProperty('--card-tint-dark')
    }
  }, [cardTint])

  return (
    <div
      id="window-root"
      className={`relative flex h-full overflow-hidden ${bgDataUrl ? 'has-bg' : ''} ${
        bgDataUrl && cardBlur ? 'card-blur' : ''
      }`}
    >
      {/* 自定义背景层（最底；四边外扩避免 blur 边缘晕白） */}
      {bgDataUrl && (
        <div className="pointer-events-none absolute inset-0 z-0 overflow-hidden">
          <img
            src={bgDataUrl}
            alt=""
            draggable={false}
            /* max-w-none 抵消 Tailwind preflight 的 max-width:100%，否则外扩宽度被钳制、右侧漏一条透明 */
            className="absolute max-w-none object-cover"
            style={{
              left: -48,
              top: -48,
              width: 'calc(100% + 96px)',
              height: 'calc(100% + 96px)',
              filter: `blur(${bgBlur}px)`,
            }}
          />
          <div className="absolute inset-0 bg-black" style={{ opacity: bgDim / 100 }} />
        </div>
      )}
      <Sidebar />
      <div className="relative z-10 flex min-w-0 flex-1 flex-col">
        <TopBar />
        {/* 内容区：磨砂或自定义背景开启时为半透明色罩，透出系统材质 / 背景图 */}
        <main
          className={`flex-1 overflow-y-auto ${
            blurEnabled || hasBg ? 'frame-content' : 'bg-neutral-100 dark:bg-neutral-950'
          }`}
        >
          {/* 页面切换淡入（key 触发重挂动画） */}
          <div key={currentView} className="h-full animate-[page-in_180ms_ease-out]">
            <View />
          </div>
        </main>
      </div>
      <DownloadPanel />
      {/* 拖放覆盖层：skin 模式 = 皮肤上传弹窗；资源管理 = 当前 Tab 类型；实例页 = 整合包 */}
      {dragActive && (
        <div className="pointer-events-none absolute inset-0 z-50 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-accent/10 backdrop-blur-sm">
          <div className="rounded-2xl border-2 border-dashed border-accent bg-white/90 px-10 py-8 text-center shadow-xl dark:bg-neutral-900/90">
            <p className="text-lg font-semibold text-accent">
              {dragMode === 'skin'
                ? '松开以上传皮肤文件'
                : currentView === 'instances'
                  ? '松开以导入整合包'
                  : (RESOURCE_CONFIGS[resourceTab as ResourceKind]?.title
                      ? `松开以安装${RESOURCE_CONFIGS[resourceTab as ResourceKind].title}`
                      : '拖放已在此选项卡停用')}
            </p>
            <p className="mt-1 text-sm text-neutral-500 dark:text-neutral-400">
              {dragMode === 'skin' ? '仅支持 PNG 格式皮肤文件' : '将安装到对应页面选中的实例'}
            </p>
          </div>
        </div>
      )}
      {/* 模态框挂载点（Modal 经 portal 定位于窗口容器内） */}
      <div id="overlay-root" />
    </div>
  )
}
