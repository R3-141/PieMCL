// Java 管理页：系统检测列表 + 各实例满足状态
//
// 数据来自 javaStore（全局共享，TopBar / 编辑对话框同源）；
// java-installed 事件触发重检测（AppShell 已全局接线）。
// 缺失的 Java 在启动对应实例时自动下载（Temurin JRE），无需手动入口；
// 手动登记的 Java 可移除登记（仅取消登记，不删除文件）。

import { useEffect, useState } from 'react'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { ipc } from '../../lib/ipc'
import { useJavaStore } from '../../stores/javaStore'
import { IconJava, IconSettings } from '../../components/common/Icons'

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
  const [adding, setAdding] = useState(false)
  const [actionError, setActionError] = useState<string | null>(null)
  const [copiedPath, setCopiedPath] = useState<string | null>(null)

  // 首次进入未检测过时拉取
  useEffect(() => {
    if (javas === null && !loading) refresh()
  }, [javas, loading, refresh])

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

  /** 移除手动登记：仅取消登记不删文件，随后重新检测刷新列表 */
  const handleRemoveCustom = async (path: string) => {
    if (!window.confirm('移除该手动登记？\n仅取消登记（不再纳入检测），不会删除文件本身。')) return
    setActionError(null)
    try {
      await ipc.removeCustomJava(path)
      await refresh()
    } catch (e) {
      setActionError(String(e))
    }
  }

  /** 复制完整路径 */
  const copyPath = async (path: string) => {
    try {
      await navigator.clipboard.writeText(path)
      setCopiedPath(path)
      window.setTimeout(() => setCopiedPath(null), 1500)
    } catch {
      /* 剪贴板不可用时忽略 */
    }
  }

  // 不满足的实例排前面（最需要关注的置顶）
  const sortedStatus = [...instanceStatus].sort((a, b) => Number(a.satisfied) - Number(b.satisfied))
  const unsatisfiedCount = instanceStatus.filter((s) => !s.satisfied).length
  const customCount = javas?.filter((j) => j.custom).length ?? 0

  const btnGhost =
    'shrink-0 rounded-md px-2 py-1 text-xs text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:text-neutral-400 dark:hover:bg-neutral-800'

  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold">Java 管理</h1>
          <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
            检测系统中的 Java 运行时并检查各实例的版本需求；启动实例时自动选择，缺失时自动下载
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

      {/* 概览统计条 */}
      {javas !== null && (
        <div className="flex items-center gap-4 rounded-xl border border-neutral-200 bg-white px-4 py-3 text-sm dark:border-neutral-800 dark:bg-neutral-900">
          <span>
            已检测 <span className="font-medium">{javas.length}</span> 个 Java
          </span>
          {customCount > 0 && (
            <span className="text-neutral-500 dark:text-neutral-400">
              {customCount} 个手动登记
            </span>
          )}
          <span className="ml-auto">
            {unsatisfiedCount > 0 ? (
              <span className="text-red-500">{unsatisfiedCount} 个实例 Java 不满足</span>
            ) : (
              instanceStatus.length > 0 && (
                <span className="text-emerald-600 dark:text-emerald-400">全部实例满足</span>
              )
            )}
          </span>
        </div>
      )}

      {error && <p className="text-sm text-red-500">{error}</p>}
      {actionError && <p className="text-sm text-red-500">{actionError}</p>}

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
            <p className="text-sm">未检测到 Java</p>
            <p className="text-xs">启动需要 Java 的实例时会自动下载；也可点击右上角「手动添加 Java」登记</p>
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
                <span className="shrink-0 rounded-md bg-neutral-100 px-1.5 py-0.5 text-xs text-neutral-600 dark:bg-neutral-800 dark:text-neutral-300">
                  {j.custom ? '手动登记' : '自动检测'}
                </span>
                <span className="hidden shrink-0 text-xs text-neutral-500 sm:inline dark:text-neutral-400">
                  {j.vendor}
                </span>
                <span
                  className="min-w-0 flex-1 truncate text-xs text-neutral-400 dark:text-neutral-500"
                  title={j.path}
                >
                  {j.path}
                </span>
                <button
                  type="button"
                  onClick={() => void copyPath(j.path)}
                  title="复制完整路径"
                  className={btnGhost}
                >
                  {copiedPath === j.path ? '已复制' : '复制路径'}
                </button>
                <button
                  type="button"
                  onClick={() => void ipc.revealPath(j.path)}
                  title="在文件管理器中打开位置"
                  className={btnGhost}
                >
                  打开位置
                </button>
                {j.custom && (
                  <button
                    type="button"
                    onClick={() => void handleRemoveCustom(j.path)}
                    title="移除手动登记（不删除文件）"
                    className="shrink-0 rounded-md px-2 py-1 text-xs text-red-400 transition-colors duration-150 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10"
                  >
                    移除登记
                  </button>
                )}
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
            {sortedStatus.map((s) => (
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
                {s.satisfied ? (
                  <span className="ml-auto">
                    <SatisfiedBadge ok />
                  </span>
                ) : (
                  <span className="ml-auto flex items-center gap-2">
                    <span className="hidden text-xs text-neutral-400 md:inline dark:text-neutral-500">
                      启动时将自动下载 Java {s.requiredJava}
                    </span>
                    <SatisfiedBadge ok={false} />
                  </span>
                )}
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
