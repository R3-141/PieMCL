// 复制实例对话框：新名称 + 是否复制存档
// 复制跳过 logs 与崩溃报告（Rust 侧实现）

import { useEffect, useState } from 'react'
import type { InstanceSummary } from '../../types/domain'
import { ipc } from '../../lib/ipc'
import { Modal } from '../../components/common/Modal'

const inputCls =
  'w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'

interface DuplicateInstanceDialogProps {
  /** 源实例；null = 关闭 */
  source: InstanceSummary | null
  onClose: () => void
}

export function DuplicateInstanceDialog({ source, onClose }: DuplicateInstanceDialogProps) {
  const [newName, setNewName] = useState('')
  const [copySaves, setCopySaves] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (source) {
      setNewName(`${source.name} 副本`)
      setCopySaves(true)
      setError(null)
    }
  }, [source])

  const handleSubmit = async () => {
    if (!source) return
    setBusy(true)
    setError(null)
    try {
      await ipc.duplicateInstance(source.id, newName.trim(), copySaves)
      onClose()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal open={source !== null} onClose={busy ? () => {} : onClose} title="复制实例">
      <div className="space-y-4">
        <p className="text-sm text-neutral-500 dark:text-neutral-400">
          基于 <span className="font-medium text-neutral-800 dark:text-neutral-200">{source?.name}</span> 创建副本，
          版本与加载器配置将完整保留；日志与崩溃报告不会复制。
        </p>
        <div>
          <label className="mb-1.5 block text-sm font-medium" htmlFor="di-name">新实例名称</label>
          <input id="di-name" className={inputCls} value={newName} onChange={(e) => setNewName(e.target.value)} disabled={busy} />
        </div>
        <label className="flex cursor-pointer items-center gap-2 text-sm text-neutral-600 dark:text-neutral-300">
          <input
            type="checkbox"
            className="size-4 accent-[var(--accent)]"
            checked={copySaves}
            onChange={(e) => setCopySaves(e.target.checked)}
            disabled={busy}
          />
          复制存档（saves）
        </label>
        {error && <p className="text-sm text-red-500">{error}</p>}
        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onClose}
            disabled={busy}
            className="rounded-lg px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:text-neutral-300 dark:hover:bg-neutral-800"
          >
            取消
          </button>
          <button
            type="button"
            onClick={handleSubmit}
            disabled={busy || newName.trim().length === 0}
            className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            {busy ? '复制中…' : '复制'}
          </button>
        </div>
      </div>
    </Modal>
  )
}
