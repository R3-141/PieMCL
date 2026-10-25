// 披风预览图（官网风格）：从 64x32 披风贴图中裁剪正面布料区域像素化显示
// 贴图坐标：正面区域位于 (1,1) 起 10x16 像素，按 4 倍缩放为 40x64 显示

import { useEffect, useState } from 'react'
import { ipc } from '../../lib/ipc'

export function CapePreview({ url, className }: { url: string; className?: string }) {
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    if (!url) {
      setFailed(true)
      return
    }
    let alive = true
    setFailed(false)
    setDataUrl(null)
    ipc
      .fetchIcon(url)
      .then((d) => {
        if (alive) setDataUrl(d)
      })
      .catch(() => {
        if (alive) setFailed(true)
      })
    return () => {
      alive = false
    }
  }, [url])

  if (failed || !dataUrl) {
    return (
      <div
        className={`flex h-16 w-10 items-center justify-center rounded bg-neutral-100 text-[10px] text-neutral-400 dark:bg-neutral-800 dark:text-neutral-500 ${className ?? ''}`}
      >
        {failed ? '×' : ''}
      </div>
    )
  }
  return (
    <div
      className={`h-16 w-10 shrink-0 rounded bg-neutral-100 dark:bg-neutral-800 ${className ?? ''}`}
      style={{
        backgroundImage: `url(${dataUrl})`,
        backgroundSize: '256px 128px', // 64x32 贴图 4 倍缩放
        backgroundPosition: '-4px -4px', // 正面区域起点 (1,1) * 4
        imageRendering: 'pixelated',
        backgroundRepeat: 'no-repeat',
      }}
      aria-label="披风预览"
    />
  )
}
