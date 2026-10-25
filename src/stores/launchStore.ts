import { create } from 'zustand'
import type { LaunchStatePayload } from '../types/domain'

/** 启动阶段类型 */
export type LaunchStage =
  | 'checking_java'
  | 'checking_resources'
  | 'downloading'
  | 'extracting_natives'
  | 'building'
  | 'starting'
  | 'running'
  | 'exited'
  | 'failed'

/** 阶段显示名 */
export const STAGE_LABEL: Record<LaunchStage, string> = {
  checking_java: '检查 Java',
  checking_resources: '检查资源',
  downloading: '补全资源',
  extracting_natives: '解压本地库',
  building: '组装启动参数',
  starting: '启动进程',
  running: '运行中',
  exited: '游戏已退出',
  failed: '启动失败',
}

/** 单实例的启动/运行视图状态（launch-* 事件驱动，零轮询） */
export interface LaunchEntry {
  instanceId: string
  stage: LaunchStage
  message: string
  pid?: number
  startedAt?: number
  /** 日志行（带 [stdout]/[stderr] 前缀；上限 1000 行 FIFO） */
  logs: string[]
  /** 可读命令摘要（启动后填充，可折叠展示） */
  command?: string
  /** 退出信息（code=null 表示被强制结束） */
  exit?: { code: number | null; summary: string | null }
}

interface LaunchStoreState {
  entries: Record<string, LaunchEntry>
  /** 最近活跃的实例（日志面板默认展示它） */
  lastActiveId: string | null
  applyState: (p: LaunchStatePayload) => void
  appendLog: (instanceId: string, stream: 'stdout' | 'stderr', line: string) => void
  applyExit: (instanceId: string, code: number | null, summary: string | null) => void
  setCommand: (instanceId: string, command: string) => void
  /** 切换日志页当前查看的实例 */
  setActive: (instanceId: string) => void
  clear: (instanceId: string) => void
}

const MAX_LOG_LINES = 1000

export const useLaunchStore = create<LaunchStoreState>()((set, get) => ({
  entries: {},
  lastActiveId: null,

  applyState: (p) => {
    const prev = get().entries[p.instanceId]
    set((s) => ({
      lastActiveId: p.instanceId,
      entries: {
        ...s.entries,
        [p.instanceId]: {
          instanceId: p.instanceId,
          stage: p.stage,
          message: p.message,
          pid: p.pid ?? prev?.pid,
          // running 阶段起计运行时长；失败/退出保留原值
          startedAt: p.stage === 'running' ? Date.now() : prev?.startedAt,
          logs: prev?.logs ?? [],
          command: prev?.command,
          exit: p.stage === 'failed' ? prev?.exit : undefined,
        },
      },
    }))
  },

  appendLog: (instanceId, stream, line) => {
    set((s) => {
      const prev = s.entries[instanceId]
      if (!prev) return s
      const logs = [...prev.logs, `[${stream}] ${line}`]
      if (logs.length > MAX_LOG_LINES) logs.splice(0, logs.length - MAX_LOG_LINES)
      return {
        entries: { ...s.entries, [instanceId]: { ...prev, logs } },
      }
    })
  },

  applyExit: (instanceId, code, summary) => {
    set((s) => {
      const prev = s.entries[instanceId]
      if (!prev) return s
      // 正常退出（0）与被强制结束（null）不算失败，仅崩溃进 failed
      const stage: LaunchStage = code === 0 || code === null ? 'exited' : 'failed'
      return {
        entries: {
          ...s.entries,
          [instanceId]: { ...prev, stage, exit: { code, summary } },
        },
      }
    })
  },

  setCommand: (instanceId, command) => {
    set((s) => {
      const prev = s.entries[instanceId]
      if (!prev) return s
      return { entries: { ...s.entries, [instanceId]: { ...prev, command } } }
    })
  },

  setActive: (instanceId) => set({ lastActiveId: instanceId }),

  clear: (instanceId) =>
    set((s) => {
      const entries = { ...s.entries }
      delete entries[instanceId]
      return { entries }
    }),
}))

/** 实例是否处于启动/运行流程中（failed 视为已结束，可重试） */
export function isBusy(entry: LaunchEntry | undefined): boolean {
  return entry !== undefined && entry.exit === undefined && entry.stage !== 'failed'
}
