// Java 管理页：系统检测列表 + 各实例满足状态 + 自动下载快捷入口
//
// 数据来自 javaStore（全局共享，TopBar / 编辑对话框同源）；
// java-installed 事件触发重检测（AppShell 已全局接线）。

import { useEffect, useState } from 'react'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { ipc } from '../../lib/ipc'
import { useJavaStore } from '../../stores/javaStore'
import { IconJava, IconPlus, IconSettings } from '../../components/common/Icons'

/** 可快捷下载的 Java 版本 */
const QUICK_DOWNLOADS = [21, 17, 8] as const

/** 满足状态徽章 */
function SatisfiedBadge({ ok }: { ok: boolean }) {
  return (
    <span
      className={`rounded-md px-1.5 py-0.5 text-xs ${
        ok
          ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300'
          : 'bg-red-100 text-red-600 dark:bg-red-500/15 dark:text-red-300'
      }`}
    >
      {ok ? '满足' : '不满足'}
    </span>
  )
}

export function JavaView() {
  const javas = useJavaStore((s) => s.javas)
  const instanceStatus = useJavaStore((s) => s.instanceStatus)
  const loading = useJavaStore((s) => s.loading)
  const error = useJavaStore((s) => s.error)
  const refresh = useJavaStore((s) => s.refresh)
  const [downloading, setDownloading] = useState<number | null>(null)
  const [adding, setAdding] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)

  // 首次进入未检测过时拉取
  useEffect(() => {
    if (javas === null && !loading) refresh()
  }, [javas, loading, refresh])

  const handleDownload = async (major: number) => {
    setActionError(null)
    setDownloading(major)
    try {
      await ipc.downloadJava(major)
      // 成功启动后由下载面板跟踪进度；任务完成后 java-installed 事件触发重检测
    } catch (e) {
      setActionError(String(e))
    } finally {
      setDownloading(null)
    }
  }

  // 手动登记 Java：选择 java.exe（或其目录），探测成功后纳入全局检测列表
  const handleAddCustom = async () => {
    setActionError(null)
    const selected = await openFileDialog({
      title: '选择 java.exe（或其所在目录）',
      multiple: false,
      directory: false,
      filters: [{ name: 'Java 可执行文件', extensions: ['exe'] }],
    })
    if (typeof selected !== 'string') return
    setAdding(true)
    try {
      await ipc.addCustomJava(selected)
      await refresh()
    } catch (e) {
      setActionError(String(e))
    } finally {
      setAdding(false)
    }
  }

  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold">Java 管理</h1>
          <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
            检测系统中的 Java 运行时，检查各实例的版本需求；缺少的版本可一键自动下载（Temurin JRE）
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={handleAddCustom}
            disabled={adding || loading}
            title="自动搜索未覆盖的位置可在此手动登记"
            className="rounded-lg border border-neutral-200 px-3 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            {adding ? '登记中…' : '手动添加 Java'}
          </button>
          <button
            type="button"
            onClick={() => refresh()}
            disabled={loading}
            className="rounded-lg border border-neutral-200 px-3 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            {loading ? '检测中…' : '重新检测'}
          </button>
        </div>
      </div>

      {error && <p className="text-sm text-red-500">{error}</p>}
      {actionError && <p className="text-sm text-red-500">{actionError}</p>}

      {/* 自动下载快捷入口 */}
      <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
        <h2 className="mb-3 text-sm font-semibold">自动下载</h2>
        <div className="flex flex-wrap gap-2">
          {QUICK_DOWNLOADS.map((major) => {
            const installed = javas?.some((j) => j.version >= major) ?? false
            return (
              <button
                key={major}
                type="button"
                disabled={downloading !== null}
                onClick={() => handleDownload(major)}
                title={
                  installed
                    ? `系统中已存在 Java ${major} 及以上版本，仍可继续下载独立的 Temurin JRE`
                    : `从 Adoptium 下载 Java ${major} 运行时`
                }
                className="flex items-center gap-1.5 rounded-lg bg-accent px-3.5 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
              >
                <IconPlus className="size-4" />
                {downloading === major ? `下载 Java ${major} 中…` : `下载 Java ${major}`}
              </button>
            )
          })}
        </div>
        <p className="mt-2 text-xs text-neutral-400 dark:text-neutral-500">
          下载进度在右侧下载面板跟踪；完成后自动解压到启动器数据目录并纳入检测
        </p>
      </section>

      {/* 检测到的 Java */}
      <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
        <h2 className="mb-3 text-sm font-semibold">系统中的 Java（{javas?.length ?? 0}）</h2>
        {javas === null ? (
          <p className="py-4 text-center text-sm text-neutral-400 dark:text-neutral-500">
            {loading ? '正在检测…' : '尚未检测'}
          </p>
        ) : javas.length === 0 ? (
          <div className="flex flex-col items-center gap-2 py-6 text-neutral-400 dark:text-neutral-500">
            <IconJava className="size-8 opacity-40" />
            <p className="text-sm">未检测到 Java，可使用上方按钮自动下载</p>
          </div>
        ) : (
          <div className="space-y-2">
            {javas.map((j) => (
              <div
                key={j.path}
                className="flex items-center gap-3 rounded-lg border border-neutral-100 px-3 py-2 dark:border-neutral-800"
              >
                <span className="shrink-0 rounded-md bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">
                  Java {j.version}
                </span>
                <span className="shrink-0 rounded-md bg-neutral-100 px-1.5 py-0.5 text-xs text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300">
                  {j.arch}
                </span>
                <span className="shrink-0 text-xs text-neutral-500 dark:text-neutral-400">{j.vendor}</span>
                <span className="truncate text-xs text-neutral-400 dark:text-neutral-500" title={j.path}>
                  {j.path}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* 各实例满足状态 */}
      <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
        <h2 className="mb-3 text-sm font-semibold">实例需求检查</h2>
        {instanceStatus.length === 0 ? (
          <p className="py-4 text-center text-sm text-neutral-400 dark:text-neutral-500">
            暂无实例
          </p>
        ) : (
          <div className="space-y-2">
            {instanceStatus.map((s) => (
              <div
                key={s.instanceId}
                className="flex items-center gap-2.5 rounded-lg border border-neutral-100 px-3 py-2 text-sm dark:border-neutral-800"
              >
                <span className="truncate font-medium" title={s.instanceName}>
                  {s.instanceName}
                </span>
                <span className="shrink-0 rounded-md bg-neutral-100 px-1.5 py-0.5 text-xs text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300">
                  {s.mcVersion}
                </span>
                <span className="shrink-0 text-xs text-neutral-500 dark:text-neutral-400">
                  需要 Java {s.requiredJava}+
                </span>
                <span className="shrink-0 text-xs text-neutral-400 dark:text-neutral-500">
                  {s.assignedPath
                    ? `已指定：Java ${s.assignedVersion ?? '?'}`
                    : '自动选择'}
                </span>
                <span className="ml-auto">
                  <SatisfiedBadge ok={s.satisfied} />
                </span>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* 兜底提示：也可手动指定 */}
      <p className="flex items-center gap-1.5 px-1 text-xs text-neutral-400 dark:text-neutral-500">
        <IconSettings className="size-3.5" />
        自动选择失败时，可在实例「编辑设置」中手动指定 Java 路径
      </p>
    </div>
  )
}
