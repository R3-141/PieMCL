// 实例详情面板：实例页右侧的主从面板
// 头部（图标 / 名称 / 徽章 / 启动与更多操作）+ 信息概览 + 设置表单
// （设置表单逻辑迁自原 EditInstanceDialog：名称 / 内存 / 预设 / 窗口 / Java / 高级参数）

import { useCallback, useEffect, useRef, useState } from 'react'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import type { InstanceDetail, InstanceSummary, UpdateInstancePatch } from '../../types/domain'
import { formatRelativeTime, formatFullTime } from '../../lib/time'
import { ipc } from '../../lib/ipc'
import { isBusy, STAGE_LABEL, useLaunchStore } from '../../stores/launchStore'
import { useJavaStore } from '../../stores/javaStore'
import { useUiStore } from '../../stores/uiStore'
import { IconFolder, IconInstances, IconMods, IconMore, IconPlay } from '../../components/common/Icons'

/** 加载器显示名 */
const LOADER_LABEL: Record<string, string> = {
  forge: 'Forge',
  fabric: 'Fabric',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
}

/** 加载器徽章配色 */
const LOADER_BADGE: Record<string, string> = {
  forge: 'bg-orange-100 text-orange-700 dark:bg-orange-500/15 dark:text-orange-300',
  fabric: 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300',
  neoforge: 'bg-red-100 text-red-700 dark:bg-red-500/15 dark:text-red-300',
  quilt: 'bg-violet-100 text-violet-700 dark:bg-violet-500/15 dark:text-violet-300',
  optifine: 'bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300',
}

const inputCls =
  'w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'
const labelCls = 'mb-1.5 block text-sm font-medium'

interface InstanceDetailPanelProps {
  instance: InstanceSummary
  onLaunch: (inst: InstanceSummary) => void
  /** 保存设置后刷新实例列表；重命名导致实例 id 变化时携带新 id 供父级重新选中 */
  onChanged: (newInstanceId?: string) => void
  onDuplicate: (inst: InstanceSummary) => void
  onDelete: (inst: InstanceSummary) => void
  onManageResources: (inst: InstanceSummary) => void
  onOpenFolder: (inst: InstanceSummary) => void
  onInstallResources: (inst: InstanceSummary) => void
  onInstallLoader: (inst: InstanceSummary) => void
  onExportModpack: (inst: InstanceSummary) => void
}

export function InstanceDetailPanel({
  instance,
  onLaunch,
  onChanged,
  onDuplicate,
  onDelete,
  onManageResources,
  onOpenFolder,
  onInstallResources,
  onInstallLoader,
  onExportModpack,
}: InstanceDetailPanelProps) {
  // ---- 头部 / 菜单 ----
  const [menuOpen, setMenuOpen] = useState(false)
  const menuRef = useRef<HTMLDivElement>(null)
  const launchEntry = useLaunchStore((s) => s.entries[instance.id])
  const launchBusy = isBusy(launchEntry)
  const setActive = useLaunchStore((s) => s.setActive)
  const setCurrentView = useUiStore((s) => s.setCurrentView)
  // Java 下拉数据（全局共享缓存；未检测过则在拉取详情时触发）
  const javas = useJavaStore((s) => s.javas)
  const javaLoading = useJavaStore((s) => s.loading)
  const refreshJavas = useJavaStore((s) => s.refresh)

  // ---- 设置表单（迁自 EditInstanceDialog）----
  const [detail, setDetail] = useState<InstanceDetail | null>(null)
  const [name, setName] = useState('')
  const [minMemoryMb, setMinMemoryMb] = useState(0)
  const [maxMemoryMb, setMaxMemoryMb] = useState(4096)
  const [preset, setPreset] = useState('default')
  const [fullscreen, setFullscreen] = useState(false)
  const [javaPath, setJavaPath] = useState<string | null>(null)
  const [widthInput, setWidthInput] = useState('')
  const [heightInput, setHeightInput] = useState('')
  const [javaTouched, setJavaTouched] = useState(false)
  const [extraArgs, setExtraArgs] = useState('')
  const [isolation, setIsolation] = useState(true)
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)

  // 实例切换时重拉详情
  useEffect(() => {
    setDetail(null)
    setError(null)
    setSaved(false)
    setJavaTouched(false)
    if (useJavaStore.getState().javas === null) {
      useJavaStore.getState().refresh()
    }
    let cancelled = false
    ipc
      .getInstance(instance.id)
      .then((d) => {
        if (cancelled) return
        setDetail(d)
        setName(d.name)
        setMinMemoryMb(d.jvm.minMemoryMb)
        setMaxMemoryMb(d.jvm.maxMemoryMb)
        setPreset(d.jvm.preset)
        setFullscreen(d.jvm.fullscreen)
        setJavaPath(d.javaPath)
        setExtraArgs(d.jvm.extraArgs)
        setIsolation(d.isolation)
        setWidthInput(d.jvm.windowWidth?.toString() ?? '')
        setHeightInput(d.jvm.windowHeight?.toString() ?? '')
      })
      .catch((e) => {
        if (!cancelled) setError(String(e))
      })
    return () => {
      cancelled = true
    }
  }, [instance.id])

  // 点击菜单外部时收起
  useEffect(() => {
    if (!menuOpen) return
    const onDocClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false)
      }
    }
    document.addEventListener('mousedown', onDocClick)
    return () => document.removeEventListener('mousedown', onDocClick)
  }, [menuOpen])

  const pickJava = useCallback(async () => {
    const selected = await openFileDialog({
      title: '选择 Java 可执行文件（javaw.exe 或 java.exe）',
      multiple: false,
      directory: false,
    })
    if (typeof selected === 'string') {
      setJavaPath(selected)
      setJavaTouched(true)
    }
  }, [])

  const selectJava = useCallback((value: string) => {
    setJavaPath(value === '' ? null : value)
    setJavaTouched(true)
  }, [])

  const handleSave = async () => {
    if (!detail) return
    if (minMemoryMb > maxMemoryMb) {
      setError('最小内存不能大于最大内存')
      return
    }
    setSaving(true)
    setError(null)
    setSaved(false)
    try {
      const patch: UpdateInstancePatch = {}
      if (name.trim() !== detail.name) patch.name = name.trim()
      if (minMemoryMb !== detail.jvm.minMemoryMb) patch.minMemoryMb = minMemoryMb
      if (maxMemoryMb !== detail.jvm.maxMemoryMb) patch.maxMemoryMb = maxMemoryMb
      if (preset !== detail.jvm.preset) patch.preset = preset
      if (fullscreen !== detail.jvm.fullscreen) patch.fullscreen = fullscreen
      if (extraArgs !== detail.jvm.extraArgs) patch.extraArgs = extraArgs
      if (javaTouched) patch.javaPath = javaPath
      if (isolation !== detail.isolation) patch.isolation = isolation
      const w = widthInput.trim() === '' ? null : Number(widthInput)
      const h = heightInput.trim() === '' ? null : Number(heightInput)
      const wChanged = (w ?? null) !== detail.jvm.windowWidth
      const hChanged = (h ?? null) !== detail.jvm.windowHeight
      if (wChanged || hChanged) {
        patch.windowWidth = w
        patch.windowHeight = h
      }
      if (Object.keys(patch).length > 0) {
        // 返回值为最新详情（重命名后 id 已变化，不能再用旧 id 重拉）
        const d2 = await ipc.updateInstanceSettings(instance.id, patch)
        setDetail(d2)
        setName(d2.name)
        setIsolation(d2.isolation)
        setJavaPath(d2.javaPath)
        setExtraArgs(d2.jvm.extraArgs)
        setMinMemoryMb(d2.jvm.minMemoryMb)
        setMaxMemoryMb(d2.jvm.maxMemoryMb)
        setPreset(d2.jvm.preset)
        setFullscreen(d2.jvm.fullscreen)
        setWidthInput(d2.jvm.windowWidth?.toString() ?? '')
        setHeightInput(d2.jvm.windowHeight?.toString() ?? '')
        // 名称变更 = 版本文件夹改名 = 实例 id 变化，通知父级刷新并重新选中
        onChanged(d2.id !== instance.id ? d2.id : undefined)
      }
      setSaved(true)
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  const loaderLabel = LOADER_LABEL[instance.loaderType]
  const menuItem =
    'flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors duration-150 hover:bg-neutral-100 dark:hover:bg-neutral-800'
  const memoryGb = (maxMemoryMb / 1024).toFixed(maxMemoryMb % 1024 === 0 ? 0 : 1)
  const overPhysical = maxMemoryMb > 8192

  return (
    <div className="flex h-full flex-col overflow-y-auto p-6 animate-[page-in_180ms_ease-out]">
      {/* 头部卡片：图标 + 名称 + 徽章 + 启动与更多操作 */}
      <div className="rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
        <div className="flex items-start gap-4">
          <div className="flex size-14 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent">
            <IconInstances className="size-8" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="truncate text-base font-semibold" title={instance.name}>
              {instance.name}
            </p>
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              <span className="rounded-md bg-neutral-100 px-1.5 py-0.5 text-xs text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300">
                {instance.mcVersion}
              </span>
              {loaderLabel && (
                <span className={`rounded-md px-1.5 py-0.5 text-xs ${LOADER_BADGE[instance.loaderType] ?? ''}`}>
                  {loaderLabel}
                  {instance.loaderVersion ? ` ${instance.loaderVersion}` : ''}
                </span>
              )}
              {instance.optifineVersion && (
                <span className={`rounded-md px-1.5 py-0.5 text-xs ${LOADER_BADGE.optifine}`}>
                  OptiFine {instance.optifineVersion}
                </span>
              )}
            </div>
          </div>
        </div>

        {/* 操作区 */}
        <div className="mt-4 flex items-center gap-2">
        {launchBusy ? (
          <button
            type="button"
            title="查看运行日志"
            onClick={() => {
              setActive(instance.id)
              setCurrentView('log')
            }}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-emerald-500 px-3 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-emerald-600"
          >
            {STAGE_LABEL[launchEntry.stage]}
          </button>
        ) : (
          <button
            type="button"
            onClick={() => onLaunch(instance)}
            className="flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
          >
            <IconPlay className="size-4" />
            启动
          </button>
        )}
        <button
          type="button"
          title="管理模组 / 数据包 / 光影包 / 资源包"
          onClick={() => onManageResources(instance)}
          className="flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          <IconMods className="size-4" />
          资源管理
        </button>
        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            title="打开文件夹"
            onClick={() => onOpenFolder(instance)}
            className="rounded-lg border border-neutral-200 p-2 text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800"
          >
            <IconFolder className="size-4" />
          </button>

          <div ref={menuRef} className="relative">
            <button
              type="button"
              aria-label="更多操作"
              onClick={() => setMenuOpen((v) => !v)}
              className="rounded-lg border border-neutral-200 p-2 text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800"
          >
            <IconMore className="size-4" />
          </button>
          {menuOpen && (
            <div className="absolute right-0 top-full z-20 mt-1 w-40 overflow-hidden rounded-lg border border-neutral-200 bg-white py-1 shadow-lg animate-[fade-in_120ms_ease-out] dark:border-neutral-700 dark:bg-neutral-800">
              <button
                type="button"
                title="将实例打包为 Modrinth 整合包（.mrpack）分享"
                className={menuItem}
                onClick={() => {
                  setMenuOpen(false)
                  onExportModpack(instance)
                }}
              >
                导出整合包
              </button>
              <button
                type="button"
                title="补全缺失或损坏的 client.jar / libraries / assets"
                className={menuItem}
                onClick={() => {
                  setMenuOpen(false)
                  onInstallResources(instance)
                }}
              >
                补全资源
              </button>
              {instance.loaderType !== 'vanilla' && (
                <button
                  type="button"
                  className={menuItem}
                  onClick={() => {
                    setMenuOpen(false)
                    onInstallLoader(instance)
                  }}
                >
                  安装加载器
                </button>
              )}
              <button
                type="button"
                className={menuItem}
                onClick={() => {
                  setMenuOpen(false)
                  onDuplicate(instance)
                }}
              >
                复制实例
              </button>
              <button
                type="button"
                className={menuItem}
                onClick={() => {
                  setMenuOpen(false)
                  onOpenFolder(instance)
                }}
              >
                <IconFolder className="size-4" />
                打开文件夹
              </button>
              <button
                type="button"
                className={`${menuItem} text-red-500 hover:bg-red-50 dark:hover:bg-red-500/10`}
                onClick={() => {
                  setMenuOpen(false)
                  onDelete(instance)
                }}
              >
                删除
              </button>
            </div>
          )}
          </div>
        </div>
      </div>
      </div>

      {/* 信息概览 */}
      <div className="mt-4 grid grid-cols-2 gap-2 text-xs text-neutral-500 dark:text-neutral-400">
        <div className="rounded-lg bg-neutral-50 px-3 py-2 dark:bg-neutral-800/60">
          <p className="text-neutral-400 dark:text-neutral-500">最后启动</p>
          <p
            className="mt-0.5 text-neutral-700 dark:text-neutral-200"
            title={instance.lastLaunchedAt ? formatFullTime(instance.lastLaunchedAt) : undefined}
          >
            {formatRelativeTime(instance.lastLaunchedAt)}
          </p>
        </div>
        <div className="rounded-lg bg-neutral-50 px-3 py-2 dark:bg-neutral-800/60">
          <p className="text-neutral-400 dark:text-neutral-500">运行状态</p>
          <p className="mt-0.5 text-neutral-700 dark:text-neutral-200">
            {launchBusy ? STAGE_LABEL[launchEntry.stage] : '未运行'}
          </p>
        </div>
      </div>

      {/* 设置表单 */}
      <div className="mt-4 space-y-4 rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
        <p className="text-sm font-semibold">实例设置</p>

        {detail === null ? (
          error ? (
            <p className="text-sm text-red-500">{error}</p>
          ) : (
            <p className="text-sm text-neutral-400">正在加载设置…</p>
          )
        ) : (
          <>
            <div>
              <label className={labelCls} htmlFor="ei-name">实例名称</label>
              <input id="ei-name" className={inputCls} value={name} onChange={(e) => setName(e.target.value)} disabled={saving} />
            </div>

            <div>
              <label className={labelCls} htmlFor="ei-iso">版本隔离</label>
              <select
                id="ei-iso"
                className={inputCls}
                value={isolation ? 'on' : 'off'}
                onChange={(e) => setIsolation(e.target.value === 'on')}
                disabled={saving}
              >
                <option value="on">开启（推荐）—— 存档、模组等存放在实例独立文件夹内</option>
                <option value="off">关闭 —— 与其他关闭隔离的实例共用 .minecraft 根目录</option>
              </select>
              <p className="mt-1 text-xs text-neutral-400">
                切换开关不会移动已有文件；开启时工作目录为 versions/{instance.name}，关闭时为 .minecraft 根（兼容个别极老模组）
              </p>
            </div>

            <div>
              <label className={labelCls} htmlFor="ei-mem">
                可分配内存：<span className="text-accent">{memoryGb} GB</span>
              </label>
              <input
                id="ei-mem"
                type="range"
                min={1024}
                max={32768}
                step={512}
                value={maxMemoryMb}
                onChange={(e) => {
                  const v = Number(e.target.value)
                  setMaxMemoryMb(v)
                  // 主流启动器语义：初始内存跟随最大内存（-Xms = -Xmx，启动即到位）
                  setMinMemoryMb(v)
                }}
                disabled={saving}
                className="w-full accent-[var(--accent)]"
              />
              <div className="flex justify-between text-xs text-neutral-400">
                <span>1 GB</span>
                <span>32 GB</span>
              </div>
              {overPhysical && (
                <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
                  可分配内存较大：建议不超过物理内存的 80%，否则游戏可能因内存不足崩溃
                </p>
              )}
            </div>

            <div>
              <label className={labelCls} htmlFor="ei-preset">JVM 优化预设</label>
              <select
                id="ei-preset"
                className={inputCls}
                value={preset}
                onChange={(e) => setPreset(e.target.value)}
                disabled={saving}
              >
                <option value="default">默认</option>
                <option value="low_spec">低配优化（减少 GC 停顿）</option>
                <option value="big_modpack">大整合包（大堆优化）</option>
              </select>
              <p className="mt-1 text-xs text-neutral-400 dark:text-neutral-500">
                选中预设后仍可在下方高级设置里追加参数，同名参数以追加的为准
              </p>
            </div>

            <div>
              <label className={labelCls}>游戏窗口</label>
              <div className="flex items-center gap-4">
                <label className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="radio"
                    name="ei-window"
                    checked={!fullscreen}
                    onChange={() => setFullscreen(false)}
                    disabled={saving}
                    className="size-4 accent-[var(--accent)]"
                  />
                  窗口模式
                </label>
                <label className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="radio"
                    name="ei-window"
                    checked={fullscreen}
                    onChange={() => setFullscreen(true)}
                    disabled={saving}
                    className="size-4 accent-[var(--accent)]"
                  />
                  全屏
                </label>
                {!fullscreen && (
                  <div className="ml-auto flex items-center gap-2 text-sm">
                    <input
                      type="number"
                      aria-label="窗口宽度"
                      placeholder="宽"
                      min={640}
                      max={7680}
                      value={widthInput}
                      onChange={(e) => setWidthInput(e.target.value)}
                      disabled={saving}
                      className="w-20 rounded-lg border border-neutral-200 bg-transparent px-2 py-1.5 text-sm outline-none focus:border-accent dark:border-neutral-700"
                    />
                    <span className="text-neutral-400">×</span>
                    <input
                      type="number"
                      aria-label="窗口高度"
                      placeholder="高"
                      min={480}
                      max={4320}
                      value={heightInput}
                      onChange={(e) => setHeightInput(e.target.value)}
                      disabled={saving}
                      className="w-20 rounded-lg border border-neutral-200 bg-transparent px-2 py-1.5 text-sm outline-none focus:border-accent dark:border-neutral-700"
                    />
                  </div>
                )}
              </div>
            </div>

            <div>
              <label className={labelCls} htmlFor="ei-java">Java 运行时</label>
              <select
                id="ei-java"
                className={inputCls}
                value={javaPath ?? ''}
                onChange={(e) => selectJava(e.target.value)}
                disabled={saving}
              >
                <option value="">自动选择（按 MC 版本匹配合适的 Java）</option>
                {javaPath !== null && !javas?.some((j) => j.path === javaPath) && (
                  <option value={javaPath}>当前指定：{javaPath}</option>
                )}
                {(javas ?? []).map((j) => (
                  <option key={j.path} value={j.path}>
                    Java {j.version}（{j.arch} · {j.vendor}）— {j.path}
                  </option>
                ))}
              </select>
              <div className="mt-1.5 flex items-center gap-2 text-xs text-neutral-400 dark:text-neutral-500">
                <span>
                  {javaLoading
                    ? '正在检测系统 Java…'
                    : javas === null
                      ? '尚未检测系统 Java'
                      : `已检测到 ${javas.length} 个运行时`}
                </span>
                <button
                  type="button"
                  onClick={refreshJavas}
                  disabled={javaLoading || saving}
                  className="text-accent transition-opacity duration-150 hover:opacity-80 disabled:opacity-50"
                >
                  重新检测
                </button>
                {javaPath !== null && (
                  <button
                    type="button"
                    onClick={() => selectJava('')}
                    disabled={saving}
                    className="text-neutral-500 transition-colors duration-150 hover:text-neutral-700 disabled:opacity-50 dark:hover:text-neutral-300"
                  >
                    恢复自动
                  </button>
                )}
                <span className="ml-auto" />
                <button
                  type="button"
                  onClick={pickJava}
                  disabled={saving}
                  className="transition-opacity duration-150 hover:opacity-80 disabled:opacity-50"
                >
                  浏览…
                </button>
              </div>
            </div>

            <div>
              <button
                type="button"
                onClick={() => setShowAdvanced((v) => !v)}
                className="text-sm text-neutral-500 transition-colors duration-150 hover:text-neutral-700 dark:hover:text-neutral-300"
              >
                {showAdvanced ? '▾ 收起高级设置' : '▸ 高级设置（JVM 参数）'}
              </button>
              {showAdvanced && (
                <textarea
                  className={`${inputCls} mt-2 h-20 resize-none font-mono text-xs animate-[fade-in_150ms_ease-out]`}
                  placeholder="附加 JVM 参数，例如 -XX:+UseG1GC"
                  value={extraArgs}
                  onChange={(e) => setExtraArgs(e.target.value)}
                  disabled={saving}
                />
              )}
            </div>

            {error && <p className="text-sm text-red-500">{error}</p>}
            {saved && !error && <p className="text-xs text-emerald-600 dark:text-emerald-400">设置已保存</p>}

            <div className="flex justify-end">
              <button
                type="button"
                onClick={handleSave}
                disabled={saving || name.trim().length === 0}
                className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
              >
                {saving ? '保存中…' : '保存设置'}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}

