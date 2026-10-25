// 实例管理视图：左列实例列表 + 右侧详情面板（主从布局）
// 数据流：挂载与 instances-changed 事件时重拉列表；
// 启动流程经 launch-* 事件推送，日志面板自动打开。

import { useCallback, useEffect, useState } from 'react'
import type { InstanceSummary } from '../../types/domain'
import { ipc, onInstancesChanged } from '../../lib/ipc'
import { ConfirmDialog } from '../../components/common/ConfirmDialog'
import { IconInstances, IconPlus } from '../../components/common/Icons'
import { useModsStore } from '../../stores/modsStore'
import { useUiStore } from '../../stores/uiStore'
import { InstanceListItem } from './InstanceListItem'
import { InstanceDetailPanel } from './InstanceDetailPanel'
import { CreateInstanceDialog } from './CreateInstanceDialog'
import { DuplicateInstanceDialog } from './DuplicateInstanceDialog'
import { ImportPackDialog } from './ImportPackDialog'
import { ExportPackDialog } from './ExportPackDialog'

export function InstancesView() {
  const [instances, setInstances] = useState<InstanceSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  // 待删除目标与执行状态
  const [deleting, setDeleting] = useState<InstanceSummary | null>(null)
  const [permanent, setPermanent] = useState(false)
  const [busy, setBusy] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  // 对话框目标
  const [createOpen, setCreateOpen] = useState(false)
  const [importOpen, setImportOpen] = useState(false)
  // 拖入的整合包路径（实例页拖入 → 打开导入对话框并预解析）
  const [importPresetPath, setImportPresetPath] = useState<string | null>(null)
  // 导出整合包目标实例
  const [exporting, setExporting] = useState<InstanceSummary | null>(null)
  const [duplicating, setDuplicating] = useState<InstanceSummary | null>(null)

  // 左列选中实例（跨页面切换保留）
  const selectedInstanceId = useUiStore((s) => s.selectedInstanceId)
  const setSelectedInstance = useUiStore((s) => s.setSelectedInstance)

  // 拖入整合包接力：AppShell 识别后写入 uiStore，此处消费并打开导入对话框
  const packDropPath = useUiStore((s) => s.packDropPath)
  useEffect(() => {
    if (packDropPath) {
      setImportPresetPath(packDropPath)
      setImportOpen(true)
      useUiStore.getState().setPackDropPath(null)
    }
  }, [packDropPath])

  const refresh = useCallback(async () => {
    try {
      setInstances(await ipc.listInstances())
      setError(null)
    } catch (e) {
      setError(String(e))
    }
  }, [])

  useEffect(() => {
    refresh()
  }, [refresh])

  // Rust 侧实例增删改后重拉
  useEffect(() => {
    let unlisten: (() => void) | undefined
    onInstancesChanged(() => refresh()).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [refresh])

  const handleDelete = async () => {
    if (!deleting) return
    setBusy(true)
    try {
      await ipc.deleteInstance(deleting.id, permanent)
      setDeleting(null)
      setPermanent(false)
    } catch (e) {
      setActionError(String(e))
    } finally {
      setBusy(false)
    }
  }

  /** 启动实例：流程与结果经 launch-* 事件推送，日志面板自动打开；成功后刷新列表（更新最后启动时间与排序） */
  const handleLaunch = async (inst: InstanceSummary) => {
    setActionError(null)
    try {
      await ipc.launchInstance(inst.id)
      await refresh()
    } catch (e) {
      setActionError(String(e))
    }
  }

  /** 补全游戏资源：成功后自动弹出下载面板跟踪进度 */
  const handleInstallResources = async (inst: InstanceSummary) => {
    setActionError(null)
    try {
      await ipc.installGameResources(inst.id)
    } catch (e) {
      setActionError(String(e))
    }
  }

  /** 安装加载器（Fabric / Quilt）：成功后自动弹出下载面板 */
  const handleInstallLoader = async (inst: InstanceSummary) => {
    setActionError(null)
    try {
      await ipc.installLoader(inst.id)
    } catch (e) {
      setActionError(String(e))
    }
  }

  const handleOpenFolder = async (inst: InstanceSummary) => {
    try {
      await ipc.openInstanceFolder(inst.id)
    } catch (e) {
      setActionError(String(e))
    }
  }

  return (
    <div className="relative flex h-full flex-col">
      {/* 头部：右上操作按钮（悬浮，与左侧卡片同行） */}
      <div className="absolute right-6 top-6 z-10 flex items-center gap-2">
        <button
          type="button"
          onClick={() => setImportOpen(true)}
          className="rounded-lg border border-neutral-200 px-3.5 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          导入整合包
        </button>
        <button
          type="button"
          onClick={() => setCreateOpen(true)}
          className="flex items-center gap-1.5 rounded-lg bg-accent px-3.5 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
        >
          <IconPlus className="size-4" />
          新建实例
        </button>
      </div>

      {/* 内容区：左列表 + 右详情（主从布局） */}
      {error ? (
        <div className="flex h-full flex-col items-center justify-center gap-3">
          <p className="text-sm text-red-500">{error}</p>
          <button
            type="button"
            onClick={refresh}
            className="rounded-lg border border-neutral-200 px-4 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            重试
          </button>
        </div>
      ) : instances === null ? (
        <div className="flex h-full items-center justify-center">
          <p className="text-sm text-neutral-400 dark:text-neutral-500">加载中…</p>
        </div>
      ) : instances.length === 0 ? (
        <div className="flex h-full flex-col items-center justify-center gap-3 text-neutral-400 dark:text-neutral-500">
          <IconInstances className="size-12 opacity-40" />
          <p className="text-sm">暂无实例，点击右上角「新建实例」开始</p>
        </div>
      ) : (
        (() => {
          // 选中回落：目标实例不存在（已删除）时取第一个
          const selected = instances.find((i) => i.id === selectedInstanceId) ?? instances[0]
          return (
            <div className="flex min-h-0 flex-1">
              {/* 左列：实例列表卡片（标题固定，列表区内滚动） */}
              <div className="flex w-72 shrink-0 flex-col pb-6 pl-6 pt-6">
                <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
                  <div className="shrink-0 px-4 pt-4">
                    <h1 className="text-lg font-semibold">实例</h1>
                    <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
                      共 {instances.length} 个实例
                    </p>
                  </div>
                  <div className="min-h-0 flex-1 overflow-y-auto p-3">
                    <div className="space-y-1">
                      {instances.map((inst) => (
                        <InstanceListItem
                          key={inst.id}
                          instance={inst}
                          selected={selected !== null && inst.id === selected.id}
                          onSelect={(target) => setSelectedInstance(target.id)}
                          onDoubleClick={handleLaunch}
                        />
                      ))}
                    </div>
                  </div>
                </div>
              </div>

              {/* 右侧：详情面板（顶部留出悬浮按钮的高度） */}
              <div className="min-w-0 flex-1 overflow-y-auto pt-[62px]">
                {selected !== null && (
                  <InstanceDetailPanel
                    key={selected.id}
                    instance={selected}
                    onLaunch={handleLaunch}
                    onChanged={(newId) => {
                      refresh()
                      // 重命名 = 实例 id 变化，重新选中新实例避免详情面板失焦
                      if (newId) setSelectedInstance(newId)
                    }}
                    onDuplicate={setDuplicating}
                    onDelete={(target) => {
                      setActionError(null)
                      setPermanent(false)
                      setDeleting(target)
                    }}
                    onManageResources={(target) => {
                      useModsStore.getState().setResourceInstance(target.id)
                      useUiStore.getState().setCurrentView('resource')
                    }}
                    onOpenFolder={handleOpenFolder}
                    onInstallResources={handleInstallResources}
                    onInstallLoader={handleInstallLoader}
                    onExportModpack={(target) => {
                      setExporting(target)
                    }}
                  />
                )}
              </div>
            </div>
          )
        })()
      )}

      {/* 新建 / 复制对话框 */}
      <CreateInstanceDialog open={createOpen} onClose={() => setCreateOpen(false)} />
      <ImportPackDialog
        open={importOpen}
        presetPath={importPresetPath}
        onClose={() => {
          setImportOpen(false)
          setImportPresetPath(null)
        }}
      />
      <DuplicateInstanceDialog source={duplicating} onClose={() => setDuplicating(null)} />
      <ExportPackDialog
        open={exporting !== null}
        instanceId={exporting?.id ?? ''}
        instanceName={exporting?.name ?? ''}
        onClose={() => setExporting(null)}
      />

      {/* 删除确认：默认回收站，可勾选彻底删除 */}
      <ConfirmDialog
        open={deleting !== null}
        title="删除实例"
        danger
        loading={busy}
        confirmText={permanent ? '彻底删除' : '移入回收站'}
        description={
          permanent ? (
            <>
              将彻底删除实例 <span className="font-medium text-neutral-900 dark:text-neutral-100">{deleting?.name}</span>：
              其存档、模组、配置与日志都会被物理删除，<span className="text-red-500">不可恢复</span>。
            </>
          ) : (
            <>
              将实例 <span className="font-medium text-neutral-900 dark:text-neutral-100">{deleting?.name}</span> 移入系统回收站，
              其存档、模组与配置会一并移走，可从回收站恢复。
            </>
          )
        }
        extra={
          <label className="flex cursor-pointer items-center gap-2 text-sm text-neutral-600 dark:text-neutral-300">
            <input
              type="checkbox"
              checked={permanent}
              onChange={(e) => setPermanent(e.target.checked)}
              className="size-4 accent-red-500"
            />
            彻底删除（跳过回收站，不可恢复）
          </label>
        }
        onConfirm={handleDelete}
        onCancel={() => {
          setDeleting(null)
          setPermanent(false)
        }}
      />

      {/* 操作失败提示 */}
      <ConfirmDialog
        open={actionError !== null}
        title="操作失败"
        confirmText="知道了"
        description={actionError}
        onConfirm={() => setActionError(null)}
        onCancel={() => setActionError(null)}
      />
    </div>
  )
}
