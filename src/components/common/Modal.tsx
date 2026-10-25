// 通用模态框：遮罩 + 居中面板，Esc / 点击遮罩关闭
// 动画 180ms（可被「减少动画」全局压缩）
// 挂载于窗口容器内的 #overlay-root（absolute 定位），避免遮罩盖到圆角外透明区

import { useEffect } from 'react'
import { createPortal } from 'react-dom'
import { IconClose } from './Icons'

interface ModalProps {
  open: boolean
  onClose: () => void
  title: string
  children: React.ReactNode
  /** 面板宽度（Tailwind max-w-* 类） */
  maxWidth?: string
  /** 点击遮罩 / 按 Esc 是否允许关闭（默认允许；设备码等关键流程传 false 防误触，
   * 此时仅右上角关闭按钮可关闭） */
  dismissible?: boolean
}

export function Modal({ open, onClose, title, children, maxWidth = 'max-w-md', dismissible = true }: ModalProps) {
  useEffect(() => {
    if (!open || !dismissible) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, onClose, dismissible])

  if (!open) return null

  return createPortal(
    <div className="absolute inset-0 z-50 flex items-center justify-center p-4">
      <div
        className="absolute inset-0 bg-black/45 animate-[fade-in_150ms_ease-out]"
        onClick={dismissible ? onClose : undefined}
      />
      <div
        role="dialog"
        aria-modal="true"
        className={`relative w-full ${maxWidth} rounded-xl border border-neutral-200 bg-white p-5 shadow-xl animate-[modal-in_180ms_ease-out] dark:border-neutral-800 dark:bg-neutral-900`}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-base font-semibold">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="关闭"
            className="rounded-md p-1 text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800 dark:hover:text-neutral-300"
          >
            <IconClose className="size-5" />
          </button>
        </div>
        {children}
      </div>
    </div>,
    document.getElementById('overlay-root') ?? document.body,
  )
}
