// 背景裁剪弹窗：壁纸设置式取景
// 16:9 视口内拖动平移 + 缩放滑块调整取景范围；确认时按视口区域从原图裁出 JPEG

import { useEffect, useRef, useState } from 'react'
import { Modal } from '../../components/common/Modal'

/** 视口宽高比（近似窗口画面比例） */
const VP_RATIO = 16 / 9
/** 输出最长边上限（控制文件体积） */
const MAX_OUT = 2560

export function BackgroundCropDialog({
  open,
  src,
  onCancel,
  onConfirm,
}: {
  open: boolean
  /** 原图 data URL */
  src: string | null
  onCancel: () => void
  onConfirm: (bytes: ArrayBuffer) => Promise<void>
}) {
  const viewportRef = useRef<HTMLDivElement>(null)
  // 原图自然尺寸与视口尺寸（cover 布局基准）
  const [nat, setNat] = useState({ w: 0, h: 0 })
  const [vp, setVp] = useState({ w: 0, h: 0 })
  // 取景状态：缩放倍率（1 = cover 铺满）与平移偏移
  const [scale, setScale] = useState(1)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const imgRef = useRef<HTMLImageElement | null>(null)

  // 打开时重置取景
  useEffect(() => {
    if (open) {
      setScale(1)
      setOffset({ x: 0, y: 0 })
      setError(null)
    }
  }, [open, src])

  // 加载原图尺寸 + 跟踪视口尺寸
  useEffect(() => {
    if (!open || !src) return
    const img = new Image()
    img.onload = () => {
      imgRef.current = img
      setNat({ w: img.naturalWidth, h: img.naturalHeight })
    }
    img.src = src
    const vpEl = viewportRef.current
    if (!vpEl) return
    const sync = () => setVp({ w: vpEl.clientWidth, h: vpEl.clientHeight })
    sync()
    const ro = new ResizeObserver(sync)
    ro.observe(vpEl)
    return () => ro.disconnect()
  }, [open, src])

  // cover 基准缩放：铺满视口所需倍率
  const base = nat.w > 0 && vp.w > 0 ? Math.max(vp.w / nat.w, vp.h / nat.h) : 1
  const dispW = nat.w * base * scale
  const dispH = nat.h * base * scale
  // 偏移限幅：图像始终覆盖视口
  const maxX = Math.max(0, (dispW - vp.w) / 2)
  const maxY = Math.max(0, (dispH - vp.h) / 2)
  const clampOffset = (o: { x: number; y: number }) => ({
    x: Math.max(-maxX, Math.min(maxX, o.x)),
    y: Math.max(-maxY, Math.min(maxY, o.y)),
  })

  // 拖动平移
  const drag = useRef({ active: false, lastX: 0, lastY: 0 })
  const onPointerDown = (e: React.PointerEvent) => {
    drag.current = { active: true, lastX: e.clientX, lastY: e.clientY }
    e.currentTarget.setPointerCapture(e.pointerId)
  }
  const onPointerMove = (e: React.PointerEvent) => {
    if (!drag.current.active) return
    const dx = e.clientX - drag.current.lastX
    const dy = e.clientY - drag.current.lastY
    drag.current.lastX = e.clientX
    drag.current.lastY = e.clientY
    setOffset((o) => clampOffset({ x: o.x + dx, y: o.y + dy }))
  }
  const onPointerUp = (e: React.PointerEvent) => {
    drag.current.active = false
    try {
      e.currentTarget.releasePointerCapture(e.pointerId)
    } catch {
      /* 指针已释放 */
    }
  }

  // 缩放变化时重 clamp 偏移（图像变大后可平移范围变大，变小后需收回）
  useEffect(() => {
    setOffset((o) => clampOffset(o))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scale, dispW, dispH])

  const handleConfirm = async () => {
    const img = imgRef.current
    if (!img || nat.w === 0 || vp.w === 0 || saving) return
    setSaving(true)
    setError(null)
    try {
      // 显示图与源图的像素比：显示尺寸 = 源尺寸 × base × scale
      const k = base * scale
      // 显示图像左上角在视口坐标系中的位置
      const left = (vp.w - dispW) / 2 + offset.x
      const top = (vp.h - dispH) / 2 + offset.y
      // 视口区域映射回源图坐标
      const sx = -left / k
      const sy = -top / k
      const sw = vp.w / k
      const sh = vp.h / k
      const outW = Math.min(MAX_OUT, Math.round(sw))
      const outH = Math.round(outW / VP_RATIO)
      const canvas = document.createElement('canvas')
      canvas.width = outW
      canvas.height = outH
      const ctx = canvas.getContext('2d')
      if (!ctx) throw new Error('无法创建画布')
      ctx.drawImage(img, sx, sy, sw, sh, 0, 0, outW, outH)
      const blob = await new Promise<Blob | null>((resolve) =>
        canvas.toBlob(resolve, 'image/jpeg', 0.85),
      )
      if (!blob) throw new Error('导出图片失败')
      await onConfirm(await blob.arrayBuffer())
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Modal open={open} onClose={onCancel} title="裁剪背景图" maxWidth="max-w-2xl">
      <div className="space-y-4">
        {/* 取景视口（拖动平移） */}
        <div
          ref={viewportRef}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          className="relative aspect-video w-full cursor-grab touch-none select-none overflow-hidden rounded-lg bg-neutral-100 active:cursor-grabbing dark:bg-neutral-800"
        >
          {src && (
            <img
              src={src}
              alt=""
              draggable={false}
              className="pointer-events-none absolute max-w-none"
              style={{
                width: dispW || undefined,
                height: dispH || undefined,
                left: vp.w ? (vp.w - dispW) / 2 + offset.x : undefined,
                top: vp.h ? (vp.h - dispH) / 2 + offset.y : undefined,
              }}
            />
          )}
          {/* 四角暗角提示取景范围 */}
          <div className="pointer-events-none absolute inset-0 ring-1 ring-inset ring-black/20" />
        </div>

        {/* 缩放滑块 */}
        <div className="flex items-center gap-3">
          <span className="w-10 shrink-0 text-xs text-neutral-500 dark:text-neutral-400">缩放</span>
          <input
            type="range"
            min={1}
            max={3}
            step={0.05}
            value={scale}
            onChange={(e) => setScale(Number(e.target.value))}
            className="flex-1 accent-[var(--accent)]"
          />
          <span className="w-12 shrink-0 text-right text-xs text-neutral-500 dark:text-neutral-400">
            {scale.toFixed(2)}x
          </span>
        </div>

        {error && (
          <p className="rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-500/10 dark:text-red-300">
            {error}
          </p>
        )}

        <p className="text-xs text-neutral-500 dark:text-neutral-400">
          拖动图片调整位置，缩放调整取景范围；确认后按 16:9 视口裁出最终背景。
        </p>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-lg px-3 py-1.5 text-sm text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
          >
            取消
          </button>
          <button
            type="button"
            onClick={handleConfirm}
            disabled={saving || nat.w === 0}
            className="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            {saving ? '保存中…' : '确认使用'}
          </button>
        </div>
      </div>
    </Modal>
  )
}
