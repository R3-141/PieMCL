// 截图选项卡：网格缩略图（懒加载 data URL）+ 放大预览 + 复制到剪贴板 /
// 打开文件位置 / 删除（两步确认，移入回收站）

import { useCallback, useEffect, useRef, useState } from 'react'
import { ipc } from '../../lib/ipc'
import { formatBytes } from '../../lib/format'
import type { ScreenshotInfo } from '../../types/domain'
import { IconCamera, IconClose, IconFolder } from '../../components/common/Icons'

function formatTimeMs(ms: number): string {
  if (ms <= 0) return '未知'
  return new Date(ms).toLocaleString()
}

export function ScreenshotsTab({ instanceId }: { instanceId: string }) {
  const [items, setItems] = useState<ScreenshotInfo[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  /** name → data URL（进入视口才加载，避免大图全量传输） */
  const [urls, setUrls] = useState<Record<string, string>>({})
  /** 预览中的截图 */
  const [preview, setPreview] = useState<ScreenshotInfo | null>(null)
  const [copied, setCopied] = useState<string | null>(null)
  /** 两步删除确认 */
  const [pendingDelete, setPendingDelete] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    try {
      setItems(await ipc.listScreenshots(instanceId))
      setUrls({})
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/i, ''))
      setItems([])
    }
  }, [instanceId])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // 两步删除确认超时还原
  useEffect(() => {
    if (pendingDelete === null) return
    const t = setTimeout(() => setPendingDelete(null), 3000)
    return () => clearTimeout(t)
  }, [pendingDelete])

  const loadUrl = useCallback(
    async (name: string) => {
      try {
        const u = await ipc.readScreenshot(instanceId, name)
        setUrls((m) => ({ ...m, [name]: u }))
      } catch {
        /* 加载失败静默：卡片显示占位图标 */
      }
    },
    [instanceId],
  )

  const handleDelete = async (item: ScreenshotInfo) => {
    if (pendingDelete !== item.name) {
      setPendingDelete(item.name)
      return
    }
    setPendingDelete(null)
    try {
      await ipc.deleteScreenshot(instanceId, item.name)
      await refresh()
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/i, ''))
    }
  }

  const handleCopy = async (item: ScreenshotInfo) => {
    try {
      await ipc.copyScreenshotToClipboard(instanceId, item.name)
      setCopied(item.name)
      setTimeout(() => setCopied(null), 1500)
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/i, ''))
    }
  }

  const handleReveal = async (item: ScreenshotInfo) => {
    try {
      await ipc.revealScreenshot(instanceId, item.name)
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/i, ''))
    }
  }

  if (error && items === null) {
    return <p className="py-10 text-center text-sm text-red-500">{error}</p>
  }

  return (
    <div>
      {error && <p className="mb-3 text-sm text-red-500">{error}</p>}
      {items === null ? (
        <p className="py-10 text-center text-sm text-neutral-400">加载中…</p>
      ) : items.length === 0 ? (
        <div className="flex flex-col items-center justify-center gap-3 py-16 text-neutral-400 dark:text-neutral-500">
          <IconCamera className="size-12 opacity-40" />
          <p className="text-sm">还没有截图</p>
          <p className="text-xs">游戏内按 F2 截图后，会出现在这里</p>
        </div>
      ) : (
        <>
          <p className="mb-3 text-xs text-neutral-400 dark:text-neutral-500">
            共 {items.length} 张 · 点击缩略图放大预览 · 删除会移入回收站
          </p>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {items.map((item) => (
              <ShotCard
                key={item.name}
                instanceId={instanceId}
                item={item}
                url={urls[item.name]}
                onLoad={loadUrl}
                copied={copied === item.name}
                pendingDelete={pendingDelete === item.name}
                onCopy={() => void handleCopy(item)}
                onReveal={() => void handleReveal(item)}
                onDelete={() => void handleDelete(item)}
                onPreview={() => setPreview(item)}
              />
            ))}
          </div>
        </>
      )}

      {/* 放大预览 */}
      {preview && (
        <div
          className="absolute inset-0 z-40 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-black/60 p-8"
          onClick={() => setPreview(null)}
        >
          <div
            className="max-w-full animate-[modal-in_150ms_ease-out]"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-2 flex items-center justify-between gap-4">
              <p className="truncate text-sm font-medium text-white">{preview.name}</p>
              <button
                type="button"
                onClick={() => setPreview(null)}
                className="flex size-7 items-center justify-center rounded-md text-white/70 transition-colors duration-150 hover:bg-white/10 hover:text-white"
              >
                <IconClose className="size-4" />
              </button>
            </div>
            {urls[preview.name] && (
              <img
                src={urls[preview.name]}
                alt={preview.name}
                draggable={false}
                className="max-h-[65vh] max-w-[60vw] rounded-lg border border-white/10 object-contain"
              />
            )}
          </div>
        </div>
      )}
    </div>
  )
}

/** 单张截图卡片：IntersectionObserver 懒加载缩略图 */
function ShotCard({
  instanceId,
  item,
  url,
  onLoad,
  copied,
  pendingDelete,
  onCopy,
  onReveal,
  onDelete,
  onPreview,
}: {
  instanceId: string
  item: ScreenshotInfo
  url: string | undefined
  onLoad: (name: string) => void
  copied: boolean
  pendingDelete: boolean
  onCopy: () => void
  onReveal: () => void
  onDelete: () => void
  onPreview: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (url) return
    const el = ref.current
    if (!el) return
    const ob = new IntersectionObserver(
      (es) => {
        if (es.some((e) => e.isIntersecting)) {
          ob.disconnect()
          onLoad(item.name)
        }
      },
      { rootMargin: '300px' },
    )
    ob.observe(el)
    return () => ob.disconnect()
  }, [item.name, url, onLoad, instanceId])

  return (
    <div ref={ref} className="group overflow-hidden rounded-xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
      {/* 缩略图（点击放大） */}
      <button
        type="button"
        onClick={onPreview}
        className="block aspect-video w-full cursor-zoom-in bg-neutral-950"
      >
        {url ? (
          <img
            src={url}
            alt={item.name}
            draggable={false}
            className="size-full object-cover transition-transform duration-150 group-hover:scale-[1.03]"
          />
        ) : (
          <div className="flex size-full items-center justify-center">
            <IconCamera className="size-8 text-neutral-600" />
          </div>
        )}
      </button>
      <div className="p-2.5">
        <p className="truncate text-xs font-medium" title={item.name}>
          {item.name}
        </p>
        <p className="mt-0.5 text-[11px] text-neutral-400 dark:text-neutral-500">
          {formatBytes(item.size)} · {formatTimeMs(item.modified)}
        </p>
        <div className="mt-2 flex items-center gap-1">
          <button
            type="button"
            onClick={onCopy}
            title="复制到剪贴板（可在画图 / 聊天窗口粘贴）"
            className={`flex-1 rounded-md px-1.5 py-1 text-[11px] transition-colors duration-150 ${
              copied
                ? 'bg-emerald-100 text-emerald-600 dark:bg-emerald-500/15 dark:text-emerald-300'
                : 'text-neutral-500 hover:bg-neutral-100 hover:text-accent dark:text-neutral-400 dark:hover:bg-neutral-800'
            }`}
          >
            {copied ? '已复制' : '复制'}
          </button>
          <button
            type="button"
            onClick={onReveal}
            title="打开文件位置"
            className="flex size-7 items-center justify-center rounded-md text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-accent dark:hover:bg-neutral-800"
          >
            <IconFolder className="size-3.5" />
          </button>
          <button
            type="button"
            onClick={onDelete}
            title={pendingDelete ? '再次点击确认删除（移入回收站）' : '删除'}
            className={`flex-1 rounded-md px-1.5 py-1 text-[11px] transition-colors duration-150 ${
              pendingDelete
                ? 'bg-red-500 font-medium text-white hover:bg-red-600'
                : 'text-neutral-400 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10'
            }`}
          >
            {pendingDelete ? '确认删除？' : '删除'}
          </button>
        </div>
      </div>
    </div>
  )
}
