// 存档选项卡：列表（icon.png 缩略图懒加载 + 名称/大小/修改时间）+
// 备份（系统保存对话框选位置 → zip）+ 删除（5 秒停留确认弹窗，移入回收站）+
// 打开存档文件夹

import { useCallback, useEffect, useRef, useState } from 'react'
import { ipc } from '../../lib/ipc'
import { formatBytes } from '../../lib/format'
import type { SaveInfo } from '../../types/domain'
import { IconArchive, IconFolder } from '../../components/common/Icons'

function formatTimeMs(ms: number): string {
  if (ms <= 0) return '未知'
  return new Date(ms).toLocaleString()
}

export function SavesTab({ instanceId }: { instanceId: string }) {
  const [items, setItems] = useState<SaveInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  /** name → icon data URL（懒加载） */
  const [icons, setIcons] = useState<Record<string, string>>({})
  /** 删除确认弹窗目标 */
  const [confirmDelete, setConfirmDelete] = useState<SaveInfo | null>(null)
  /** 备份成功提示（显示保存路径） */
  const [backupMsg, setBackupMsg] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    try {
      setItems(await ipc.listSaves(instanceId))
      setIcons({})
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/i, ''))
      setItems([])
    }
  }, [instanceId])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const loadIcon = useCallback(
    async (name: string) => {
      try {
        const u = await ipc.readSaveIcon(instanceId, name)
        if (u) setIcons((m) => ({ ...m, [name]: u }))
      } catch {
        /* 加载失败静默：显示占位图标 */
      }
    },
    [instanceId],
  )

  const handleBackup = async (s: SaveInfo) => {
    setBusy(true)
    setError(null)
    try {
      const path = await ipc.backupSave(instanceId, s.name)
      setBackupMsg(path)
    } catch (e) {
      const msg = String(e).replace(/^Error:\s*/i, '')
      if (!msg.includes('已取消')) setError(msg)
    } finally {
      setBusy(false)
    }
  }

  const handleDelete = async (s: SaveInfo) => {
    setBusy(true)
    setError(null)
    try {
      await ipc.deleteSave(instanceId, s.name)
      setConfirmDelete(null)
      await refresh()
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/i, ''))
      setBusy(false)
    } finally {
      setBusy(false)
    }
  }

  if (error && items === null) {
    return <p className="py-10 text-center text-sm text-red-500">{error}</p>
  }

  return (
    <div>
      {error && <p className="mb-3 text-sm text-red-500">{error}</p>}
      {backupMsg && (
        <div className="mb-3 flex items-center justify-between gap-3 rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300">
          <span className="truncate" title={backupMsg}>
            备份完成：{backupMsg}
          </span>
          <button
            type="button"
            onClick={() => setBackupMsg(null)}
            className="text-xs opacity-60 hover:opacity-100"
          >
            关闭
          </button>
        </div>
      )}
      {items === null ? (
        <p className="py-10 text-center text-sm text-neutral-400">加载中…</p>
      ) : items.length === 0 ? (
        <div className="flex flex-col items-center justify-center gap-3 py-16 text-neutral-400 dark:text-neutral-500">
          <IconArchive className="size-12 opacity-40" />
          <p className="text-sm">该实例还没有存档</p>
          <p className="text-xs">创建世界后，可在这里备份与管理存档</p>
        </div>
      ) : (
        <>
          <div className="mb-3 flex items-center justify-between">
            <p className="text-xs text-neutral-400 dark:text-neutral-500">
              共 {items.length} 个存档 · 删除会移入回收站 · 备份为 zip 压缩包
            </p>
            <button
              type="button"
              onClick={() => void ipc.openSavesDir(instanceId)}
              className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-1.5 text-xs transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            >
              <IconFolder className="size-3.5" />
              打开存档文件夹
            </button>
          </div>
          <div className="space-y-2">
            {items.map((s) => (
              <SaveCard
                key={s.name}
                instanceId={instanceId}
                save={s}
                icon={icons[s.name]}
                onLoadIcon={loadIcon}
                busy={busy}
                onBackup={() => void handleBackup(s)}
                onDelete={() => setConfirmDelete(s)}
              />
            ))}
          </div>
        </>
      )}

      {/* 删除确认弹窗（5 秒停留后才能确认） */}
      {confirmDelete && (
        <DeleteSaveConfirm
          save={confirmDelete}
          busy={busy}
          onConfirm={() => void handleDelete(confirmDelete)}
          onClose={() => setConfirmDelete(null)}
        />
      )}
    </div>
  )
}

/** 存档卡片：icon.png 缩略图懒加载 */
function SaveCard({
  instanceId,
  save,
  icon,
  onLoadIcon,
  busy,
  onBackup,
  onDelete,
}: {
  instanceId: string
  save: SaveInfo
  icon: string | undefined
  onLoadIcon: (name: string) => void
  busy: boolean
  onBackup: () => void
  onDelete: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!save.hasIcon || icon) return
    const el = ref.current
    if (!el) return
    const ob = new IntersectionObserver(
      (es) => {
        if (es.some((e) => e.isIntersecting)) {
          ob.disconnect()
          onLoadIcon(save.name)
        }
      },
      { rootMargin: '200px' },
    )
    ob.observe(el)
    return () => ob.disconnect()
  }, [save.name, save.hasIcon, icon, onLoadIcon, instanceId])

  return (
    <div
      ref={ref}
      className="flex items-center gap-3 rounded-xl border border-neutral-200 bg-white px-4 py-3 dark:border-neutral-800 dark:bg-neutral-900"
    >
      {/* 缩略图（MC 自动生成的 icon.png，缺省显示占位） */}
      <div className="size-14 shrink-0 overflow-hidden rounded-lg bg-neutral-100 dark:bg-neutral-800">
        {icon ? (
          <img
            src={icon}
            alt={save.name}
            draggable={false}
            className="size-full object-cover [image-rendering:pixelated]"
          />
        ) : (
          <div className="flex size-full items-center justify-center">
            <IconArchive className="size-6 text-neutral-400" />
          </div>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium" title={save.name}>
          {save.name}
        </p>
        <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
          {formatBytes(save.size)} · 最后游玩 {formatTimeMs(save.modified)}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          onClick={onBackup}
          disabled={busy}
          title="选择保存位置，打包为 zip 压缩包"
          className="rounded-lg border border-neutral-200 px-3 py-1.5 text-xs transition-colors duration-150 hover:border-accent hover:text-accent disabled:opacity-50 dark:border-neutral-700"
        >
          备份
        </button>
        <button
          type="button"
          onClick={onDelete}
          disabled={busy}
          className="rounded-lg px-2.5 py-1.5 text-xs text-neutral-400 transition-colors duration-150 hover:bg-red-50 hover:text-red-500 disabled:opacity-50 dark:hover:bg-red-500/10"
        >
          删除
        </button>
      </div>
    </div>
  )
}

/** 存档删除确认弹窗：按钮 5 秒停留倒计时后才能点击确认 */
function DeleteSaveConfirm({
  save,
  busy,
  onConfirm,
  onClose,
}: {
  save: SaveInfo
  busy: boolean
  onConfirm: () => void
  onClose: () => void
}) {
  const [countdown, setCountdown] = useState(5)

  useEffect(() => {
    if (countdown <= 0) return
    const t = setTimeout(() => setCountdown((c) => c - 1), 1000)
    return () => clearTimeout(t)
  }, [countdown])

  return (
    <div
      className="absolute inset-0 z-40 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="w-[22rem] animate-[modal-in_150ms_ease-out] rounded-xl border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-800 dark:bg-neutral-900"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="text-sm font-semibold">删除存档</h3>
        <div className="mt-3 space-y-2 text-sm text-neutral-600 dark:text-neutral-300">
          <p>
            确定要删除存档 <b>{save.name}</b> 吗？
          </p>
          <p className="rounded-lg border border-red-200 bg-red-50 p-2.5 text-xs leading-5 text-red-600 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-300">
            存档大小 {formatBytes(save.size)}，删除后将<b>移入系统回收站</b>（可在回收站找回，
            但不建议长期依赖）。备份一份更稳妥。
          </p>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            取消
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={countdown > 0 || busy}
            className="rounded-lg bg-red-500 px-4 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-red-600 disabled:opacity-50"
          >
            {countdown > 0 ? `请阅读（${countdown}s 后可删除）` : busy ? '删除中…' : '确认删除'}
          </button>
        </div>
      </div>
    </div>
  )
}
