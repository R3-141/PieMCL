import { create } from 'zustand'
import type { DownloadProgress } from '../types/domain'
import { useUiStore } from './uiStore'

/** 下载任务的前端视图状态（由 download-progress 事件驱动，零轮询） */
export interface DownloadTaskView {
  taskId: string
  kind: string
  /** 关联实例 id 或资源锁键（Java 下载为 java:{major}） */
  instanceId: string | null
  state: 'running' | 'paused' | 'completed' | 'failed' | 'cancelled'
  currentFile: string | null
  completedFiles: number
  totalFiles: number
  downloadedBytes: number
  totalBytes: number
  bytesPerSecond: number
  error: string | null
  updatedAt: number
  /** 首次出现时间（排序依据：进度刷新不改变位置，避免行上下跳动） */
  createdAt: number
}

interface DownloadStoreState {
  /** 任务表（含运行中与终态，终态由用户清除） */
  tasks: Record<string, DownloadTaskView>
  /** 收到新任务时自动弹出面板；终态失败时也弹出 */
  applyProgress: (p: DownloadProgress) => void
  removeTask: (taskId: string) => void
  clearFinished: () => void
}

/** 任务类型显示名 */
export const KIND_LABEL: Record<string, string> = {
  resources: '资源补全',
  loader: '加载器安装',
  java: 'Java 下载',
  mods: '模组安装',
  pack: '整合包导入',
}

const TERMINAL = new Set(['completed', 'failed', 'cancelled'])

export const useDownloadStore = create<DownloadStoreState>()((set, get) => ({
  tasks: {},

  applyProgress: (p) => {
    const prev = get().tasks[p.taskId]
    const createdAt = prev?.createdAt ?? Date.now()
    set((s) => ({
      tasks: {
        ...s.tasks,
        [p.taskId]: {
          taskId: p.taskId,
          kind: p.kind,
          instanceId: p.instanceId,
          state: p.state as DownloadTaskView['state'],
          currentFile: p.currentFile,
          completedFiles: p.completedFiles,
          totalFiles: p.totalFiles,
          downloadedBytes: p.downloadedBytes,
          totalBytes: p.totalBytes,
          bytesPerSecond: p.bytesPerSecond,
          error: p.error,
          updatedAt: Date.now(),
          createdAt,
        },
      },
    }))
    // 新任务自动展开下载面板；失败也立即提醒
    if (!prev || p.state === 'failed') {
      useUiStore.getState().setDownloadPanelVisible(true)
    }
  },

  removeTask: (taskId) =>
    set((s) => {
      const tasks = { ...s.tasks }
      delete tasks[taskId]
      return { tasks }
    }),

  clearFinished: () =>
    set((s) => ({
      tasks: Object.fromEntries(
        Object.entries(s.tasks).filter(([, t]) => !TERMINAL.has(t.state)),
      ),
    })),
}))
