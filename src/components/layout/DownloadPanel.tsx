// 下载进度面板：右侧滑出抽屉
// 任务进度由 download-progress 事件驱动（零轮询）；面板关闭即最小化到顶部状态栏

import { ipc } from '../../lib/ipc'
import { formatSpeed } from '../../lib/format'
import { KIND_LABEL, useDownloadStore, type DownloadTaskView } from '../../stores/downloadStore'
import { useUiStore } from '../../stores/uiStore'
import { IconClose } from '../common/Icons'

/** 单个任务行 */
function TaskRow({ task }: { task: DownloadTaskView }) {
  const removeTask = useDownloadStore((s) => s.removeTask)
  // 操作状态（暂停/继续/取消的执行中与错误）
  const act = async (fn: (id: string) => Promise<void>) => {
    try {
      await fn(task.taskId)
    } catch {
      // 错误不影响面板（例如任务已结束），静默忽略
    }
  }

  const running = task.state === 'running'
  const paused = task.state === 'paused'
  const terminal = task.state === 'completed' || task.state === 'failed' || task.state === 'cancelled'

  // 进度：字节未知时按文件数
  const bytePct = task.totalBytes > 0 ? Math.min(100, Math.round((task.downloadedBytes / task.totalBytes) * 100)) : null
  const filePct = task.totalFiles > 0 ? Math.round((task.completedFiles / task.totalFiles) * 100) : 0

  const stateBadge = {
    running: 'bg-accent-soft text-accent',
    paused: 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300',
    completed: 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300',
    failed: 'bg-red-100 text-red-600 dark:bg-red-500/15 dark:text-red-300',
    cancelled: 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400',
  }[task.state]

  const stateLabel = { running: '下载中', paused: '已暂停', completed: '已完成', failed: '失败', cancelled: '已取消' }[task.state]

  return (
    <div className="animate-[item-in_150ms_ease-out] rounded-xl border border-neutral-200 bg-white p-3 dark:border-neutral-800 dark:bg-neutral-900">
      {/* 标题行 */}
      <div className="flex items-center gap-2">
        <span className="truncate text-sm font-medium">{task.currentFile ?? KIND_LABEL[task.kind] ?? task.kind}</span>
        <span className={`ml-auto shrink-0 rounded-md px-1.5 py-0.5 text-xs ${stateBadge}`}>{stateLabel}</span>
        {terminal && (
          <button
            type="button"
            title="清除"
            onClick={() => removeTask(task.taskId)}
            className="shrink-0 rounded-md p-1 text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800"
          >
            <IconClose className="size-3.5" />
          </button>
        )}
      </div>

      {/* 进度条 */}
      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-neutral-100 dark:bg-neutral-800">
        <div
          className={`h-full rounded-full transition-[width] duration-300 ${task.state === 'failed' ? 'bg-red-400' : 'bg-accent'}`}
          style={{ width: `${bytePct ?? filePct}%` }}
        />
      </div>

      {/* 摘要行 */}
      <div className="mt-1.5 flex items-center gap-2 text-xs text-neutral-400 dark:text-neutral-500">
        <span>
          {bytePct !== null ? `${bytePct}%` : `文件 ${task.completedFiles}/${task.totalFiles}`}
        </span>
        {bytePct !== null && task.totalFiles > 0 && <span>· 文件 {task.completedFiles}/{task.totalFiles}</span>}
        {running && <span className="ml-auto text-accent">{formatSpeed(task.bytesPerSecond)}</span>}
      </div>

      {/* 当前文件（未完成时显示） */}
      {running && task.currentFile && (
        <p className="mt-1 truncate text-xs text-neutral-400 dark:text-neutral-500" title={task.currentFile}>
          {task.currentFile}
        </p>
      )}

      {/* 失败原因 */}
      {task.state === 'failed' && task.error && (
        <p className="mt-1.5 break-all rounded-lg bg-red-50 px-2 py-1.5 text-xs text-red-600 dark:bg-red-500/10 dark:text-red-300">
          {task.error}
        </p>
      )}

      {/* 操作区（含失败重试） */}
      {task.state === 'failed' ? (
        <div className="mt-2 flex gap-2">
          <button
            type="button"
            onClick={() => act(ipc.retryDownload)}
            className="rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
          >
            重试
          </button>
          <button
            type="button"
            onClick={() => removeTask(task.taskId)}
            className="rounded-lg border border-neutral-200 px-2.5 py-1 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
          >
            移除
          </button>
        </div>
      ) : (
        !terminal && (
          <div className="mt-2 flex gap-2">
            {running && (
              <button
                type="button"
                onClick={() => act(ipc.pauseDownload)}
                className="rounded-lg border border-neutral-200 px-2.5 py-1 text-xs transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                暂停
              </button>
            )}
            {paused && (
              <button
                type="button"
                onClick={() => act(ipc.resumeDownload)}
                className="rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
              >
                继续
              </button>
            )}
            <button
              type="button"
              onClick={() => act(ipc.cancelDownload)}
              className="rounded-lg border border-neutral-200 px-2.5 py-1 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
            >
              取消
            </button>
          </div>
        )
      )}
    </div>
  )
}

/** 右侧滑出下载队列抽屉（关闭 = 最小化到顶部状态栏；定位于窗口容器内，不遮挡标题栏三键） */
export function DownloadPanel() {
  const visible = useUiStore((s) => s.downloadPanelVisible)
  const setVisible = useUiStore((s) => s.setDownloadPanelVisible)
  const tasks = useDownloadStore((s) => s.tasks)
  const clearFinished = useDownloadStore((s) => s.clearFinished)

  // 固定按创建时间倒序（新任务在上）——不能用 updatedAt 排序，
  // 否则并行下载时每条进度刷新都会重排，行会上下乱跳
  const list = Object.values(tasks).sort((a, b) => b.createdAt - a.createdAt)
  const hasFinished = list.some(
    (t) => t.state === 'completed' || t.state === 'failed' || t.state === 'cancelled',
  )

  return (
    <div
      style={{
        position: 'absolute',
        top: 48,
        right: 0,
        bottom: 0,
        width: 384,
        flexShrink: 0,
        transform: visible ? 'translateX(0)' : 'translateX(100%)',
        transition: 'transform 200ms',
      }}
      className="z-40 flex flex-col border-l border-neutral-200 bg-neutral-50 shadow-xl dark:border-neutral-800 dark:bg-neutral-950"
      aria-hidden={!visible}
    >
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-neutral-200 px-4 dark:border-neutral-800">
        <span className="text-sm font-semibold">下载队列</span>
        <div className="flex items-center gap-1">
          {hasFinished && (
            <button
              type="button"
              onClick={clearFinished}
              className="rounded-lg px-2.5 py-1.5 text-xs text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
            >
              清除已完成
            </button>
          )}
          <button
            type="button"
            title="最小化到状态栏"
            onClick={() => setVisible(false)}
            className="rounded-lg p-1.5 text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
          >
            <IconClose className="size-4" />
          </button>
        </div>
      </div>

      <div className="flex-1 space-y-2.5 overflow-y-auto p-3">
        {list.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-neutral-400 dark:text-neutral-500">
            <p>暂无下载任务</p>
            <p className="text-xs">补全资源、安装加载器与下载 Java 的进度都会在这里显示</p>
          </div>
        ) : (
          list.map((t) => <TaskRow key={t.taskId} task={t} />)
        )}
      </div>
    </div>
  )
}
