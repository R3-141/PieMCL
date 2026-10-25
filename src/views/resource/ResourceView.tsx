// 实例资源管理页（实例附属页，从实例卡片「更多 → 资源管理」进入）
//
// 绑定单个实例，管理其模组 / 数据包 / 光影包 / 资源包：
// 类型 Tab 切换 + 每 Tab 的已安装列表 / 市场 / 回收站 / 更新检查。
// 类型差异（启停 / 导出 / 依赖警告 / 文案 / 落位目录）由 ResourceConfig 驱动；
// 拖放安装目标 = 本页绑定的实例 + 当前 Tab 类型（AppShell 全局接线按此分流）。

import { useCallback, useEffect, useRef, useState } from 'react'
import { ipc, onInstancesChanged, onModsChanged } from '../../lib/ipc'
import { formatBytes } from '../../lib/format'
import { useModsStore, type ResourceTab } from '../../stores/modsStore'
import type { InstanceSummary, ModInfo, TrashItem, UpdateInfo } from '../../types/domain'
import { RESOURCE_CONFIGS, type ResourceKind } from '../community/config'
import { IconArchive, IconCamera, IconFolder } from '../../components/common/Icons'
import { ScreenshotsTab } from './ScreenshotsTab'
import { SavesTab } from './SavesTab'

const LOADER_LABEL: Record<string, string> = {
  fabric: 'Fabric',
  forge: 'Forge',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
  vanilla: '原版',
}

const ORIGIN_LABEL: Record<string, string> = {
  modrinth: 'Modrinth',
  curseforge: 'CurseForge',
  local: '本地',
}

/** 类型 Tab 顺序（资源四类 + 截图 + 存档媒体管理） */
const TAB_ORDER: readonly ResourceTab[] = [
  'mods',
  'datapacks',
  'shaders',
  'resourcepacks',
  'screenshots',
  'saves',
]

/** 截图 / 存档 Tab 的图标与标题（非资源类型，不在 RESOURCE_CONFIGS 中） */
const MEDIA_TAB_META: Record<string, { icon: typeof IconCamera; title: string }> = {
  screenshots: { icon: IconCamera, title: '截图' },
  saves: { icon: IconArchive, title: '存档' },
}

function formatTime(epochSec: number): string {
  if (epochSec <= 0) return '未知'
  return new Date(epochSec * 1000).toLocaleString()
}

export function ResourceView({ instanceId, onBack }: { instanceId: string; onBack: () => void }) {
  const resourceTab = useModsStore((s) => s.resourceTab)
  const setResourceTab = useModsStore((s) => s.setResourceTab)
  const dropMessage = useModsStore((s) => s.dropMessage)
  const setDropMessage = useModsStore((s) => s.setDropMessage)

  const cfg = RESOURCE_CONFIGS[resourceTab as ResourceKind]
  const [instance, setInstance] = useState<InstanceSummary | null>(null)
  const [items, setItems] = useState<ModInfo[] | null>(null)
  const [trash, setTrash] = useState<TrashItem[]>([])
  const [showTrash, setShowTrash] = useState(false)
  const [updates, setUpdates] = useState<UpdateInfo[] | null>(null)
  const [checking, setChecking] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  /** 正在后台更新的文件名集合（防重复点击） */
  const updatingRef = useRef(new Set<string>())

  const refreshInstance = useCallback(async () => {
    try {
      const list = await ipc.listInstances()
      const found = list.find((i) => i.id === instanceId) ?? null
      setInstance(found)
      return found
    } catch (e) {
      setError(String(e))
      return null
    }
  }, [instanceId])

  const refreshItems = useCallback(async () => {
    // 截图 / 存档为媒体管理 Tab，无资源列表
    if (resourceTab === 'screenshots' || resourceTab === 'saves') return
    try {
      setItems(await ipc.listMods(instanceId, cfg.resourceType))
    } catch (e) {
      setError(String(e))
    }
  }, [instanceId, cfg?.resourceType, resourceTab])

  const refreshTrash = useCallback(async () => {
    try {
      setTrash(await ipc.listTrash(instanceId))
    } catch (e) {
      setError(String(e))
    }
  }, [instanceId])

  // 初始拉取实例信息；实例被删除时自动踢回实例列表
  useEffect(() => {
    void refreshInstance().then((found) => {
      if (!found) {
        useModsStore.getState().setDropMessage('该实例已被删除，资源管理页已关闭')
        onBack()
      }
    })
    let unlisten: (() => void) | undefined
    onInstancesChanged(() => {
      void refreshInstance().then((found) => {
        if (!found) {
          useModsStore.getState().setDropMessage('该实例已被删除，资源管理页已关闭')
          onBack()
        }
      })
    }).then((u) => (unlisten = u))
    return () => unlisten?.()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId])

  // 切类型 / 实例变更 → 刷新当前类型列表
  useEffect(() => {
    setItems(null)
    setMessage(null)
    setError(null)
    setUpdates(null)
    refreshItems()
    if (showTrash) refreshTrash()
  }, [refreshItems, refreshTrash, showTrash])

  // mods-changed 事件：后端任何变更（拖放 / 其他操作）后刷新当前实例
  useEffect(() => {
    let unlisten: (() => void) | undefined
    onModsChanged((id) => {
      if (id === instanceId) {
        refreshItems()
        if (showTrash) refreshTrash()
      }
    }).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [instanceId, refreshItems, refreshTrash, showTrash])

  const handleToggle = async (m: ModInfo) => {
    setError(null)
    try {
      await ipc.toggleMod(instanceId, cfg.resourceType, m.fileName, !m.enabled)
      await refreshItems()
    } catch (e) {
      setError(String(e))
    }
  }

  const handleDelete = async (m: ModInfo) => {
    const confirmMsg = cfg.supportsTrash
      ? `删除「${m.name}」？\n文件将移入实例回收站，可在回收站中恢复。`
      : `删除「${m.name}」？\n文件将被永久删除，无法恢复。`
    if (!window.confirm(confirmMsg)) return
    setError(null)
    try {
      await ipc.deleteMods(instanceId, cfg.resourceType, [m.fileName])
      setMessage(`已删除：${m.name}`)
      await refreshItems()
      if (showTrash) await refreshTrash()
    } catch (e) {
      setError(String(e))
    }
  }

  const handleRestore = async (item: TrashItem) => {
    setError(null)
    try {
      await ipc.restoreMod(instanceId, cfg.resourceType, item.trashName)
      setMessage(`已恢复：${item.originalName}`)
      await Promise.all([refreshItems(), refreshTrash()])
    } catch (e) {
      setError(String(e))
    }
  }

  const handleEmptyTrash = async () => {
    if (!window.confirm('清空回收站？被删除的文件将无法恢复。')) return
    setError(null)
    try {
      const n = await ipc.emptyTrash(instanceId)
      setMessage(`已清空回收站（${n} 个文件）`)
      await refreshTrash()
    } catch (e) {
      setError(String(e))
    }
  }

  const handleCheckUpdates = async () => {
    setChecking(true)
    setError(null)
    try {
      const list = await ipc.checkModUpdates(instanceId, cfg.resourceType)
      setUpdates(list)
      setMessage(list.length === 0 ? `所有${cfg.title}均为最新版本` : null)
    } catch (e) {
      setError(String(e))
    } finally {
      setChecking(false)
    }
  }

  const handleUpdate = (u: UpdateInfo) => {
    if (updatingRef.current.has(u.fileName)) return
    updatingRef.current.add(u.fileName)
    setError(null)
    setMessage(`正在更新「${u.title}」…`)
    void (async () => {
      try {
        await ipc.confirmModInstall(instanceId, cfg.resourceType, [u.newVersionId], [u.fileName])
        setMessage(`已更新：${u.title} → ${u.newVersionNumber}`)
        setUpdates((list) => list?.filter((x) => x.fileName !== u.fileName) ?? null)
        await refreshItems()
      } catch (e) {
        setError(String(e))
      } finally {
        updatingRef.current.delete(u.fileName)
      }
    })()
  }

  /** 导出整合包入口已移至实例详情「更多 → 导出整合包」 */

  // 缺失依赖汇总（仅模组；仅启用中的条目）
  const missingAll = new Set<string>()
  if (cfg?.showsLoaderMeta) {
    items?.forEach((m) => {
      if (m.enabled) m.missingDeps.forEach((d) => missingAll.add(d))
    })
  }

  const btnGhost =
    'rounded-lg px-3 py-1.5 text-sm text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:text-neutral-400 dark:hover:bg-neutral-800'

  return (
    <div className="flex h-full flex-col">
      {/* 头部：返回 + 实例环境 + 操作 */}
      <div className="flex flex-wrap items-center gap-3 px-6 pt-6">
        <button
          type="button"
          onClick={onBack}
          className="rounded-lg px-2.5 py-1.5 text-sm text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
        >
          ← 返回实例
        </button>
        <h1 className="text-lg font-semibold">资源管理</h1>
        {instance && (
          <span className="text-xs text-neutral-500 dark:text-neutral-400">
            {instance.name} · MC {instance.mcVersion}
            {instance.loaderType !== 'vanilla' &&
              ` · ${LOADER_LABEL[instance.loaderType] ?? instance.loaderType}`}
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          {cfg && !showTrash && (
            <button
              type="button"
              onClick={handleCheckUpdates}
              disabled={checking}
              className={btnGhost}
            >
              {checking ? '检查中…' : '检查更新'}
            </button>
          )}
          {cfg && (
            <button type="button" onClick={() => void refreshItems()} className={btnGhost}>
              刷新
            </button>
          )}
          {cfg?.supportsTrash && (
            <button
              type="button"
              onClick={() => setShowTrash((v) => !v)}
              className={`rounded-lg px-3 py-1.5 text-sm transition-colors duration-150 ${
                showTrash
                  ? 'bg-accent-soft font-medium text-accent'
                  : btnGhost
              }`}
            >
              回收站
            </button>
          )}
          <button
            type="button"
            title="打开实例文件夹（游戏数据目录）"
            onClick={() => void ipc.openInstanceFolder(instanceId)}
            className={btnGhost}
          >
            <IconFolder className="size-4" />
          </button>
        </div>
      </div>

      {/* 类型 Tab */}
      <div className="flex flex-wrap items-center gap-2 px-6 pt-4">
        {TAB_ORDER.map((kind) => {
          const media = MEDIA_TAB_META[kind]
          const c = RESOURCE_CONFIGS[kind as ResourceKind]
          const icon = media?.icon ?? c.icon
          const title = media?.title ?? c.title
          const active = kind === resourceTab
          return (
            <button
              key={kind}
              type="button"
              onClick={() => {
                setResourceTab(kind)
                setShowTrash(false)
              }}
              className={`flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm transition-colors duration-150 ${
                active
                  ? 'bg-accent-soft font-medium text-accent'
                  : 'text-neutral-500 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800'
              }`}
            >
              {(() => {
                const Icon = icon
                return <Icon className="size-4" />
              })()}
              {title}
            </button>
          )
        })}
        {!MEDIA_TAB_META[resourceTab] && (
          <p className="ml-auto text-xs text-neutral-400 dark:text-neutral-500">
            在线安装请到侧栏「模组市场」
          </p>
        )}
      </div>

      <main className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6 pt-4">
        {/* key 随类型变化重挂，触发切换淡入 */}
        <div key={resourceTab} className="animate-[page-in_150ms_ease-out]">
        {resourceTab === 'screenshots' ? (
          /* ---------------- 截图媒体管理 ---------------- */
          <ScreenshotsTab instanceId={instanceId} />
        ) : resourceTab === 'saves' ? (
          /* ---------------- 存档媒体管理 ---------------- */
          <SavesTab instanceId={instanceId} />
        ) : (
        <>
        {/* 提示区 */}
        {dropMessage && (
          <div className="mb-3 flex items-center justify-between gap-3 rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300">
            <span>{dropMessage}</span>
            <button
              type="button"
              onClick={() => setDropMessage(null)}
              className="text-xs opacity-60 hover:opacity-100"
            >
              关闭
            </button>
          </div>
        )}
        {message && (
          <div className="mb-3 rounded-lg bg-neutral-100 px-3 py-2 text-sm text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300">
            {message}
          </div>
        )}
        {error && (
          <div className="mb-3 rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
            {error}
          </div>
        )}
        {cfg.notice && !showTrash && (
          <div className="mb-3 rounded-lg bg-sky-50 px-3 py-2 text-xs text-sky-700 dark:bg-sky-500/10 dark:text-sky-300">
            {cfg.notice}
          </div>
        )}
        {instance?.loaderType === 'vanilla' && cfg.vanillaNote && !showTrash && (
          <div className="mb-3 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700 dark:bg-amber-500/10 dark:text-amber-300">
            {cfg.vanillaNote}
          </div>
        )}

        {showTrash ? (
          /* ---------------- 回收站视图（仅模组）---------------- */
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <p className="text-sm text-neutral-500 dark:text-neutral-400">
                已删除的{cfg.title}保留在实例 .trash 目录中，可在保留期内恢复（回收站为实例级共用）
              </p>
              {trash.length > 0 && (
                <button
                  type="button"
                  onClick={handleEmptyTrash}
                  className="rounded-lg border border-red-200 px-3 py-1.5 text-xs text-red-500 transition-colors duration-150 hover:bg-red-50 dark:border-red-500/30 dark:hover:bg-red-500/10"
                >
                  清空回收站
                </button>
              )}
            </div>
            {trash.length === 0 ? (
              <p className="py-10 text-center text-sm text-neutral-400 dark:text-neutral-500">
                回收站是空的
              </p>
            ) : (
              trash.map((t) => (
                <div
                  key={t.trashName}
                  className="flex items-center gap-3 rounded-xl border border-neutral-200 bg-white px-4 py-3 dark:border-neutral-800 dark:bg-neutral-900"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm">{t.originalName}</p>
                    <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
                      删除于 {formatTime(t.deletedAt)} · {formatBytes(t.fileSize)}
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={() => handleRestore(t)}
                    className="rounded-lg bg-accent-soft px-3 py-1.5 text-xs font-medium text-accent transition-colors duration-150 hover:bg-accent-hover hover:text-white"
                  >
                    恢复
                  </button>
                </div>
              ))
            )}
          </div>
        ) : (
          /* ---------------- 已安装列表 ---------------- */
          <>
            {updates && updates.length > 0 && (
              <div className="mb-3 space-y-1.5 rounded-xl border border-emerald-200 bg-emerald-50/60 p-3 dark:border-emerald-500/30 dark:bg-emerald-500/5">
                <p className="text-xs font-medium text-emerald-700 dark:text-emerald-300">
                  发现 {updates.length} 个可更新{cfg.title}
                </p>
                {updates.map((u) => (
                  <div key={u.fileName} className="flex items-center gap-2 text-sm">
                    <span className="min-w-0 flex-1 truncate">{u.title}</span>
                    <span className="shrink-0 font-mono text-xs text-neutral-400">
                      {u.currentVersionNumber} → {u.newVersionNumber}
                    </span>
                    <button
                      type="button"
                      onClick={() => handleUpdate(u)}
                      className="shrink-0 rounded-md bg-emerald-600 px-2.5 py-1 text-xs font-medium text-white transition-colors duration-150 hover:bg-emerald-500"
                    >
                      更新
                    </button>
                  </div>
                ))}
              </div>
            )}
            {missingAll.size > 0 && (
              <div className="mb-3 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700 dark:bg-amber-500/10 dark:text-amber-300">
                {missingAll.size} 个前置依赖缺失（{[...missingAll].join('、')}
                ）。缺失可能导致游戏启动失败，可到「{cfg.title}市场」搜索安装。
              </div>
            )}
            {items === null ? (
              <p className="py-10 text-center text-sm text-neutral-400 dark:text-neutral-500">加载中…</p>
            ) : items.length === 0 ? (
              <div className="flex flex-col items-center justify-center gap-3 py-16 text-neutral-400 dark:text-neutral-500">
                <cfg.icon className="size-12 opacity-40" />
                <p className="text-sm">{cfg.emptyTitle}</p>
                <p className="text-xs">{cfg.emptyHint}</p>
              </div>
            ) : (
              <div className="space-y-2">
                {items.map((m) => (
                  <div
                    key={m.fileName}
                    className="rounded-xl border border-neutral-200 bg-white px-4 py-3 transition-colors duration-150 dark:border-neutral-800 dark:bg-neutral-900"
                  >
                    <div className="flex items-center gap-3">
                      {/* 启用开关（仅模组） */}
                      {cfg.supportsToggle && (
                        <input
                          type="checkbox"
                          checked={m.enabled}
                          onChange={() => handleToggle(m)}
                          aria-label={m.enabled ? '禁用' : '启用'}
                          className="size-4 shrink-0 accent-[var(--accent)]"
                        />
                      )}
                      {/* 名称 + 版本 */}
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span
                            className={`truncate text-sm ${
                              cfg.supportsToggle && !m.enabled
                                ? 'text-neutral-400 line-through dark:text-neutral-500'
                                : ''
                            }`}
                          >
                            {m.name}
                          </span>
                          {m.version && (
                            <span className="text-xs text-neutral-400 dark:text-neutral-500">
                              {m.version}
                            </span>
                          )}
                          {cfg.showsLoaderMeta && m.loader && (
                            <span className="rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400">
                              {LOADER_LABEL[m.loader] ?? m.loader}
                            </span>
                          )}
                          {m.source && (
                            <span
                              className={`rounded px-1.5 py-0.5 text-[10px] ${
                                m.source.origin === 'local'
                                  ? 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
                                  : 'bg-accent-soft text-accent'
                              }`}
                            >
                              {ORIGIN_LABEL[m.source.origin] ?? m.source.origin}
                            </span>
                          )}
                        </div>
                        <p className="mt-0.5 truncate text-xs text-neutral-400 dark:text-neutral-500">
                          {m.fileName} · {formatBytes(m.fileSize)}
                          {cfg.showsLoaderMeta && m.enabled && m.missingDeps.length > 0 && (
                            <span className="text-amber-600 dark:text-amber-400">
                              {' '}
                              · 缺少前置：{m.missingDeps.join('、')}
                            </span>
                          )}
                        </p>
                      </div>
                      {/* 删除 */}
                      <button
                        type="button"
                        onClick={() => handleDelete(m)}
                        className="shrink-0 rounded-lg px-2.5 py-1.5 text-xs text-neutral-400 transition-colors duration-150 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10"
                      >
                        删除
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </>
        )}
        </>
        )}
        </div>
      </main>
    </div>
  )
}
