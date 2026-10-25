// 危险操作确认对话框：文案必须明确写出影响范围

import { Modal } from './Modal'

interface ConfirmDialogProps {
  open: boolean
  title: string
  /** 影响范围说明（支持多行/富文本） */
  description: React.ReactNode
  confirmText?: string
  cancelText?: string
  /** 危险操作（确认按钮红色） */
  danger?: boolean
  /** 确认执行中（禁用按钮 + 文案） */
  loading?: boolean
  /** 额外内容（如「彻底删除」勾选框） */
  extra?: React.ReactNode
  onConfirm: () => void
  onCancel: () => void
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmText = '确认',
  cancelText = '取消',
  danger = false,
  loading = false,
  extra,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Modal open={open} onClose={loading ? () => {} : onCancel} title={title}>
      <div className="text-sm leading-relaxed text-neutral-600 dark:text-neutral-300">
        {description}
      </div>
      {extra && <div className="mt-3">{extra}</div>}
      <div className="mt-5 flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          disabled={loading}
          className="rounded-lg px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          {cancelText}
        </button>
        <button
          type="button"
          onClick={onConfirm}
          disabled={loading}
          className={`rounded-lg px-4 py-2 text-sm font-medium text-white transition-colors duration-150 disabled:opacity-50 ${
            danger ? 'bg-red-500 hover:bg-red-600' : 'bg-accent hover:bg-accent-hover'
          }`}
        >
          {loading ? '处理中…' : confirmText}
        </button>
      </div>
    </Modal>
  )
}
