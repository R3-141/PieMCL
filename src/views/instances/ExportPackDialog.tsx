// 导出整合包对话框：PCL2 风格「导出内容列表」勾选树 → 选保存位置 → 生成 .mrpack
// 内容清单由 Rust 按实例 minecraft/ 目录扫描（list_export_items），勾选键回传 export_modpack

import { useCallback, useEffect, useState } from 'react'
import { ipc } from '../../lib/ipc'
import type { ExportGroup } from '../../types/domain'

export function ExportPackDialog({
  open,
  instanceId,
  instanceName,
  onClose,
}: {
  open: boolean
  instanceId: string
  instanceName: string
  onClose: () => void
}) {
  const [groups, setGroups] = useState<ExportGroup[] | null>(null)
  const [checked, setChecked] = useState<Set<string>>(new Set())
  const [error, setError] = useState<string | null>(null)
  const [exporting, setExporting] = useState(false)
  const [done, setDone] = useState<string | null>(null)

  // 打开时加载内容清单并按默认勾选初始化
  useEffect(() => {
    if (!open) return
    setGroups(null)
    setDone(null)
    setError(null)
    setExporting(false)
    ipc
      .listExportItems(instanceId)
      .then((g) => {
        setGroups(g)
        const init = new Set<string>()
        for (const grp of g) {
          for (const item of grp.items) {
            if (item.defaultChecked) init.add(item.key)
          }
        }
        setChecked(init)
      })
      .catch((e) => setError(String(e)))
  }, [open, instanceId])

  const toggle = useCallback((key: string) => {
    setChecked((prev) => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }, [])

  /** 组头勾选：全选 / 全不选该组子项 */
  const toggleGroup = useCallback((grp: ExportGroup) => {
    setChecked((prev) => {
      const next = new Set(prev)
      const allOn = grp.items.every((it) => next.has(it.key))
      for (const it of grp.items) {
        if (allOn) next.delete(it.key)
        else next.add(it.key)
      }
      return next
    })
  }, [])

  const handleExport = async () => {
    if (exporting) return
    setExporting(true)
    setError(null)
    try {
      const dest = await ipc.exportModpack(instanceId, [...checked])
      setDone(dest)
    } catch (e) {
      const msg = String(e)
      if (msg.includes('已取消导出')) {
        // 用户在保存对话框点了取消：回到勾选界面
      } else {
        setError(msg)
      }
    } finally {
      setExporting(false)
    }
  }

  if (!open) return null

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/40 p-6">
      <div className="flex max-h-[85%] w-full max-w-lg flex-col rounded-2xl border border-neutral-200 bg-white p-5 shadow-2xl dark:border-neutral-800 dark:bg-neutral-900">
        {done ? (
          <>
            <h3 className="text-base font-semibold">导出完成</h3>
            <p className="mt-3 break-all rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300">
              已保存到：{done}
            </p>
            <div className="mt-4 flex justify-end">
              <button
                type="button"
                onClick={onClose}
                className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
              >
                完成
              </button>
            </div>
          </>
        ) : (
          <>
            <h3 className="text-base font-semibold">导出整合包</h3>
            <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
              Mod 本体（含 Modrinth 下载清单与本地模组）始终包含；以下为可选导出内容。
            </p>

            {/* 导出内容列表 */}
            <div className="mt-3 min-h-0 flex-1 space-y-1 overflow-y-auto rounded-lg border border-neutral-100 p-3 dark:border-neutral-800">
              {groups === null ? (
                <p className="py-6 text-center text-sm text-neutral-400">正在扫描实例内容…</p>
              ) : (
                groups.map((grp) => {
                  const allOn = grp.items.length > 0 && grp.items.every((it) => checked.has(it.key))
                  const someOn = grp.items.some((it) => checked.has(it.key))
                  return (
                    <div key={grp.key}>
                      {/* 组头 */}
                      <div className="flex items-center gap-2 py-1">
                        {grp.required ? (
                          <span
                            className="flex size-4 items-center justify-center rounded border border-neutral-300 bg-neutral-100 text-[10px] text-neutral-400 dark:border-neutral-700 dark:bg-neutral-800"
                            title="始终包含"
                          >
                            ✓
                          </span>
                        ) : (
                          <input
                            type="checkbox"
                            checked={allOn}
                            ref={(el) => {
                              if (el) el.indeterminate = !allOn && someOn
                            }}
                            onChange={() => toggleGroup(grp)}
                            disabled={grp.items.length === 0}
                            className="size-4 accent-[var(--accent)]"
                          />
                        )}
                        <span className="text-sm font-medium">{grp.label}</span>
                      </div>
                      {/* 子项 */}
                      <div className="ml-6">
                        {grp.items.map((it) => (
                          <label
                            key={it.key}
                            className="flex cursor-pointer items-center gap-2 py-0.5 text-sm"
                          >
                            <input
                              type="checkbox"
                              checked={checked.has(it.key)}
                              onChange={() => toggle(it.key)}
                              className="size-3.5 accent-[var(--accent)]"
                            />
                            <span>{it.label}</span>
                            {it.hint && (
                              <span className="truncate text-xs text-neutral-400 dark:text-neutral-500">
                                {it.hint}
                              </span>
                            )}
                          </label>
                        ))}
                      </div>
                    </div>
                  )
                })
              )}
            </div>

            {error && (
              <p className="mt-3 break-all rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-500/10 dark:text-red-300">
                {error}
              </p>
            )}

            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={onClose}
                disabled={exporting}
                className="rounded-lg bg-neutral-100 px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
              >
                取消
              </button>
              <button
                type="button"
                onClick={() => void handleExport()}
                disabled={exporting || groups === null}
                className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
              >
                {exporting ? '导出中…' : '导出'}
              </button>
            </div>
            <p className="mt-2 text-[10px] text-neutral-400 dark:text-neutral-500">
              导出「{instanceName}」为 Modrinth 整合包（.mrpack）；点击导出后选择保存位置。
            </p>
          </>
        )}
      </div>
    </div>
  )
}
