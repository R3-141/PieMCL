// 更新提示弹窗：启动 8 秒后静默检查 GitHub Releases，发现新版本时弹出。
// 用户可选：前往 GitHub 下载 / 跳过此版本（持久化）/ 不再提醒（关闭启动检查）。
// 检查失败完全静默（国内网络 GitHub 常不可达，不打扰用户）。

import { useEffect, useState } from 'react'
import { ipc } from '../../lib/ipc'
import type { UpdateCheckResult } from '../../types/domain'
import { Modal } from './Modal'

const STARTUP_CHECK_DELAY_MS = 8_000

export function UpdatePromptModal() {
  const [result, setResult] = useState<UpdateCheckResult | null>(null)
  const [open, setOpen] = useState(false)

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void ipc
        .checkUpdate(false)
        .then((r) => {
          if (r.hasUpdate) {
            setResult(r)
            setOpen(true)
          }
        })
        .catch(() => {
          /* 静默失败：启动检查不打扰用户 */
        })
    }, STARTUP_CHECK_DELAY_MS)
    return () => window.clearTimeout(timer)
  }, [])

  if (!result) return null

  /** 跳过此版本：记录版本号，启动检查不再弹窗（手动检查仍可见） */
  const skipVersion = async () => {
    try {
      const cur = await ipc.getSettings()
      await ipc.saveSettings({ ...cur, updateSkippedVersion: result.latestVersion })
    } catch {
      /* 保存失败仅关闭弹窗，下次启动可能再提醒 */
    }
    setOpen(false)
  }

  /** 不再提醒：关闭启动检查（设置页手动检查仍可用） */
  const disableChecks = async () => {
    try {
      const cur = await ipc.getSettings()
      await ipc.saveSettings({ ...cur, updateCheckEnabled: false })
    } catch {
      /* 同上 */
    }
    setOpen(false)
  }

  return (
    <Modal open={open} onClose={() => setOpen(false)} title="发现新版本" dismissible>
      <div className="space-y-3">
        <p className="text-sm text-neutral-600 dark:text-neutral-300">
          PieMCL <span className="font-medium">{result.latestVersion}</span> 已发布
          （当前 v{result.currentVersion}），请前往 GitHub 下载更新。
        </p>
        {result.notes.trim() && (
          <div className="max-h-56 overflow-y-auto rounded-lg bg-neutral-100 px-3 py-2 text-xs whitespace-pre-wrap text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300">
            {result.notes}
          </div>
        )}
        <div className="flex flex-col gap-2">
          <button
            type="button"
            onClick={() => void ipc.openExternal(result.htmlUrl)}
            className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
          >
            前往 GitHub 下载
          </button>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => void skipVersion()}
              className="flex-1 rounded-lg bg-neutral-100 px-3 py-2 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
            >
              跳过此版本
            </button>
            <button
              type="button"
              onClick={() => void disableChecks()}
              className="flex-1 rounded-lg bg-neutral-100 px-3 py-2 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
            >
              不再提醒
            </button>
          </div>
        </div>
      </div>
    </Modal>
  )
}
