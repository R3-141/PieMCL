// 运行日志视图：正在运行/最近启动实例的阶段状态、可折叠命令摘要、实时日志流
// 数据来自 launchStore（launch-* 事件驱动，零轮询）；内存/时长在页面可见时轮询刷新

import { useEffect, useRef, useState } from 'react'
import { ipc } from '../../lib/ipc'
import { formatBytes } from '../../lib/format'
import { isBusy, STAGE_LABEL, useLaunchStore } from '../../stores/launchStore'
import { IconLogs } from '../../components/common/Icons'

/** 毫秒时长 → 可读文本 */
function formatUptime(ms: number): string {
  const s = Math.floor(ms / 1000)
  const m = Math.floor(s / 60)
  const h = Math.floor(m / 60)
  if (h > 0) return `${h} 时 ${m % 60} 分`
  if (m > 0) return `${m} 分 ${s % 60} 秒`
  return `${s} 秒`
}

export function LogView() {
  const entries = useLaunchStore((s) => s.entries)
  const lastActiveId = useLaunchStore((s) => s.lastActiveId)
  const setActive = useLaunchStore((s) => s.setActive)
  const clear = useLaunchStore((s) => s.clear)
  const [showCommand, setShowCommand] = useState(false)
  const [memoryBytes, setMemoryBytes] = useState<number | null>(null)
  const logEndRef = useRef<HTMLDivElement>(null)

  // 有日志记录的实例列表（用于多实例切换）
  const recordedIds = Object.keys(entries)
  const viewId = lastActiveId && entries[lastActiveId] ? lastActiveId : recordedIds[0]
  const entry = viewId ? entries[viewId] : undefined
  const busy = isBusy(entry)

  // 页面可见且进程运行中：每 3 秒轮询内存
  useEffect(() => {
    if (!busy || !entry) return
    let stopped = false
    const poll = async () => {
      try {
        const list = await ipc.listRunning()
        if (stopped) return
        const mine = list.find((r) => r.instanceId === entry.instanceId)
        setMemoryBytes(mine?.memoryBytes ?? null)
      } catch {
        /* 轮询失败静默 */
      }
    }
    poll()
    const timer = window.setInterval(poll, 3000)
    return () => {
      stopped = true
      window.clearInterval(timer)
    }
  }, [busy, entry?.instanceId, entry])

  // 新日志自动滚动到底
  useEffect(() => {
    logEndRef.current?.scrollIntoView({ block: 'end' })
  }, [entry?.logs.length])

  const handleKill = async () => {
    if (!entry) return
    try {
      await ipc.killInstance(entry.instanceId)
    } catch (e) {
      // 进程可能已退出，静默；状态由 launch-exited 收尾
      console.error('强制结束失败：', e)
    }
  }

  return (
    <div className="flex h-full flex-col">
      {/* 头部 */}
      <div className="flex items-center justify-between px-6 pt-6">
        <div>
          <h1 className="text-lg font-semibold">运行日志</h1>
          {recordedIds.length > 1 && (
            <div className="mt-2 flex gap-1.5">
              {recordedIds.map((id) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setActive(id)}
                  className={`max-w-40 truncate rounded-md px-2 py-1 text-xs transition-colors duration-150 ${
                    id === viewId
                      ? 'bg-accent-soft font-medium text-accent'
                      : 'bg-neutral-100 text-neutral-500 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-400 dark:hover:bg-neutral-700'
                  }`}
                  title={id}
                >
                  {id}
                </button>
              ))}
            </div>
          )}
        </div>
        {entry && !busy && (
          <button
            type="button"
            onClick={() => clear(entry.instanceId)}
            className="rounded-lg px-3 py-2 text-sm text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
          >
            清除记录
          </button>
        )}
      </div>

      {!entry ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-neutral-400 dark:text-neutral-500">
          <IconLogs className="size-12 opacity-40" />
          <p className="text-sm">暂无启动记录</p>
          <p className="text-xs">点击实例卡片「启动」后，这里会实时显示启动状态与游戏日志</p>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-3 p-6">
          {/* 状态卡 */}
          <div className="rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
            <div className="flex flex-wrap items-center gap-2">
              <span
                className={`rounded-md px-2 py-0.5 text-xs ${
                  entry.stage === 'running'
                    ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300'
                    : entry.stage === 'failed'
                      ? 'bg-red-100 text-red-600 dark:bg-red-500/15 dark:text-red-300'
                      : entry.stage === 'exited'
                        ? 'bg-neutral-100 text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300'
                        : 'bg-accent-soft text-accent'
                }`}
              >
                {STAGE_LABEL[entry.stage]}
              </span>
              {entry.stage === 'running' && entry.startedAt && (
                <span className="text-xs text-neutral-400 dark:text-neutral-500">
                  已运行 {formatUptime(Date.now() - entry.startedAt)}
                </span>
              )}
              {entry.stage === 'running' && memoryBytes !== null && (
                <span className="text-xs text-neutral-400 dark:text-neutral-500">
                  · 内存 {formatBytes(memoryBytes)}
                </span>
              )}
              {entry.stage === 'running' && entry.pid && (
                <span className="text-xs text-neutral-400 dark:text-neutral-500">
                  · PID {entry.pid}
                </span>
              )}
              {busy && (
                <button
                  type="button"
                  onClick={handleKill}
                  className="ml-auto rounded-lg border border-red-200 px-3 py-1.5 text-xs text-red-500 transition-colors duration-150 hover:bg-red-50 dark:border-red-500/30 dark:hover:bg-red-500/10"
                >
                  强制结束
                </button>
              )}
            </div>
            <p className="mt-1.5 text-xs text-neutral-400 dark:text-neutral-500">{entry.message}</p>
            {entry.exit?.summary && (
              <p
                className={`mt-2 break-all rounded-lg px-2 py-1.5 text-xs ${
                  entry.stage === 'failed'
                    ? 'bg-red-50 text-red-600 dark:bg-red-500/10 dark:text-red-300'
                    : 'bg-neutral-100 text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300'
                }`}
              >
                {entry.exit.summary}
                {entry.exit.code !== null && `（退出码 ${entry.exit.code}）`}
              </p>
            )}
          </div>

          {/* 命令摘要（可折叠） */}
          {entry.command && (
            <div>
              <button
                type="button"
                onClick={() => setShowCommand((v) => !v)}
                className="text-xs text-neutral-500 transition-colors duration-150 hover:text-neutral-700 dark:text-neutral-400 dark:hover:text-neutral-300"
              >
                {showCommand ? '▾ 收起启动参数' : '▸ 启动参数（高级）'}
              </button>
              {showCommand && (
                <pre className="mt-1 max-h-40 overflow-auto break-all rounded-lg bg-neutral-100 p-3 font-mono text-[11px] leading-relaxed text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300">
                  {entry.command}
                </pre>
              )}
            </div>
          )}

          {/* 日志区（占满剩余高度） */}
          <div className="min-h-0 flex-1 overflow-y-auto rounded-xl bg-neutral-950 p-3 font-mono text-xs leading-relaxed text-neutral-300">
            {entry.logs.length === 0 ? (
              <p className="text-neutral-500">等待日志输出…</p>
            ) : (
              entry.logs.map((l, i) => (
                <p key={i} className="break-all whitespace-pre-wrap">
                  {l}
                </p>
              ))
            )}
            <div ref={logEndRef} />
          </div>
        </div>
      )}
    </div>
  )
}
