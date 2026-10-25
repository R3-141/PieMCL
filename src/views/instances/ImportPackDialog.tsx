// 整合包导入对话框：选文件（Rust 侧对话框）→ 预览确认 → 导入（进度事件展示）
// 进度：pack-import-progress 事件（阶段文案）；下载明细走下载面板（download-progress）

import { useCallback, useEffect, useRef, useState } from 'react'
import { ipc, onPackImportProgress } from '../../lib/ipc'
import type { PackPreview } from '../../types/domain'

const inputCls =
  'w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'

const KIND_LABEL: Record<string, string> = {
  modrinth: 'Modrinth (.mrpack)',
  curseforge: 'CurseForge (zip)',
}

export function ImportPackDialog({
  open,
  presetPath,
  onClose,
}: {
  open: boolean
  /** 预置整合包路径（拖入导入用）：非空时打开后直接解析该文件，不弹文件选择器 */
  presetPath?: string | null
  onClose: () => void
}) {
  const [preview, setPreview] = useState<PackPreview | null>(null)
  const [name, setName] = useState('')
  const [path, setPath] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [progress, setProgress] = useState<string | null>(null)
  const [done, setDone] = useState<string | null>(null)
  const [importing, setImporting] = useState(false)
  const unlistenRef = useRef<(() => void) | null>(null)
  // onClose 经 ref 传递：effect 只依赖 open，避免父组件重渲染（新函数引用）
  // 触发 effect 重跑、取消还在等待的文件选择调用（否则预览永远无法返回）
  const onCloseRef = useRef(onClose)
  onCloseRef.current = onClose

  /** 弹出文件选择并解析预览；拖入路径（presetPath）存在时直接解析该文件 */
  const openPicker = useCallback(() => {
    setError(null)
    setPreview(null)
    const task = presetPath ? ipc.parsePackFile(presetPath) : ipc.pickModpack()
    task
      .then((p) => {
        if (!p) {
          // 仅文件选择取消（返回 null）时关闭；拖入解析恒有结果
          onCloseRef.current()
          return
        }
        setPreview(p)
        setPath(p.path ?? null)
        setName(p.name)
        // pack-preview 与 pick 返回合并：path 单独存
      })
      .catch((e) => {
        setError(String(e))
      })
  }, [presetPath])

  // 打开时立即弹文件选择；取消则关闭对话框
  useEffect(() => {
    if (!open) return
    setProgress(null)
    setDone(null)
    setImporting(false)
    openPicker()
  }, [open, openPicker])

  // 导入期间接进度事件
  useEffect(() => {
    if (!importing) return
    let alive = true
    onPackImportProgress((p) => {
      if (!alive) return
      setProgress(p.message ?? p.stage)
    }).then((u) => {
      unlistenRef.current = u
    })
    return () => {
      alive = false
      unlistenRef.current?.()
      unlistenRef.current = null
    }
  }, [importing])

  if (!open) return null

  const start = async () => {
    if (!path || !name.trim()) return
    setImporting(true)
    setError(null)
    setProgress('正在解析整合包…')
    try {
      const iid = await ipc.importModpack(path, name.trim())
      // 短暂展示成功提示后自动关闭（实例列表经 instances-changed 事件自动刷新）
      setDone(`导入完成：${iid}（模组 / 配置 / 资源已就绪）`)
      setTimeout(() => {
        onCloseRef.current()
      }, 1200)
    } catch (e) {
      setError(String(e))
      setImporting(false)
      setProgress(null)
    }
  }

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/40 p-6">
      <div className="w-full max-w-md rounded-2xl bg-white p-5 shadow-2xl dark:bg-neutral-900">
        <h3 className="text-base font-semibold">导入整合包</h3>

        {done ? (
          <>
            <p className="mt-3 rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300">
              {done}
            </p>
          </>
        ) : preview ? (
          <>
            {/* 预览信息 */}
            <div className="mt-3 space-y-2 rounded-xl bg-neutral-50 p-3 text-sm dark:bg-neutral-800/60">
              <div className="flex justify-between gap-3">
                <span className="text-neutral-500 dark:text-neutral-400">来源</span>
                <span>{KIND_LABEL[preview.kind] ?? preview.kind}</span>
              </div>
              <div className="flex justify-between gap-3">
                <span className="text-neutral-500 dark:text-neutral-400">Minecraft 版本</span>
                <span>{preview.mcVersion}</span>
              </div>
              <div className="flex justify-between gap-3">
                <span className="text-neutral-500 dark:text-neutral-400">加载器</span>
                <span>
                  {preview.loaderType === 'vanilla'
                    ? '无（原版）'
                    : `${preview.loaderType} ${preview.loaderVersion ?? ''}`}
                </span>
              </div>
              <div className="flex justify-between gap-3">
                <span className="text-neutral-500 dark:text-neutral-400">模组数</span>
                <span>{preview.modCount}</span>
              </div>
            </div>
            <div className="mt-3">
              <label className="mb-1.5 block text-sm font-medium" htmlFor="pack-name">
                实例名称
              </label>
              <input
                id="pack-name"
                className={inputCls}
                value={name}
                onChange={(e) => setName(e.target.value)}
                disabled={importing}
              />
            </div>
            {progress && (
              <p className="mt-3 text-xs text-neutral-500 dark:text-neutral-400">{progress}</p>
            )}
            {error && (
              <p className="mt-3 break-all rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-500/10 dark:text-red-300">
                {error}
              </p>
            )}
            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={onClose}
                disabled={importing}
                className="rounded-lg bg-neutral-100 px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
              >
                {importing ? '后台继续' : '取消'}
              </button>
              <button
                type="button"
                onClick={() => void start()}
                disabled={importing || !name.trim()}
                className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
              >
                {importing ? '导入中…' : '开始导入'}
              </button>
            </div>
          </>
        ) : error ? (
          /* 选择/解析失败：明确展示错误并支持重试 */
          <>
            <p className="mt-3 break-all rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-500/10 dark:text-red-300">
              {error}
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={onCloseRef.current}
                className="rounded-lg bg-neutral-100 px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
              >
                关闭
              </button>
              <button
                type="button"
                onClick={openPicker}
                className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
              >
                重新选择
              </button>
            </div>
          </>
        ) : (
          <p className="mt-4 py-6 text-center text-sm text-neutral-400">正在打开文件选择器…</p>
        )}
      </div>
    </div>
  )
}
