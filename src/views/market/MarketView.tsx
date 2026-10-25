// 模组市场页（侧栏入口）：类型 Tab（模组 / 数据包 / 光影包 / 资源包）
// + 安装目标实例选择器 + 市场（Modrinth / CurseForge）。
// 安装与拖放均以「选中的实例 + 当前类型 Tab」为准。

import { useCallback, useEffect, useState } from 'react'
import { ipc, onInstancesChanged } from '../../lib/ipc'
import { useModsStore } from '../../stores/modsStore'
import type { InstanceSummary } from '../../types/domain'
import { RESOURCE_CONFIGS, type ResourceKind } from '../community/config'
import { MarketPanel } from '../community/MarketPanel'

/** 类型 Tab 顺序 */
const TAB_ORDER: readonly ResourceKind[] = ['mods', 'datapacks', 'shaders', 'resourcepacks']

export function MarketView() {
  const marketTab = useModsStore((s) => s.marketTab)
  const setMarketTab = useModsStore((s) => s.setMarketTab)
  const marketInstanceId = useModsStore((s) => s.marketInstanceId)
  const setMarketInstance = useModsStore((s) => s.setMarketInstance)

  const cfg = RESOURCE_CONFIGS[marketTab]
  const [instances, setInstances] = useState<InstanceSummary[] | null>(null)
  const instance = instances?.find((i) => i.id === marketInstanceId) ?? null

  const refreshInstances = useCallback(async () => {
    try {
      const list = await ipc.listInstances()
      setInstances(list)
      // 首次进入或目标实例被删时，回落到第一个实例
      if (!list.some((i) => i.id === useModsStore.getState().marketInstanceId)) {
        useModsStore.getState().setMarketInstance(list[0]?.id ?? null)
      }
    } catch {
      /* 列表加载失败：保持现状 */
    }
  }, [])

  useEffect(() => {
    void refreshInstances()
    let unlisten: (() => void) | undefined
    onInstancesChanged(() => refreshInstances()).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [refreshInstances])

  return (
    <div className="flex h-full flex-col">
      {/* 头部：标题 + 类型 Tab + 安装目标实例 */}
      <div className="flex flex-wrap items-center gap-3 px-6 pt-6">
        <h1 className="text-lg font-semibold">模组市场</h1>
        <div className="flex flex-wrap items-center gap-1.5">
          {TAB_ORDER.map((kind) => {
            const c = RESOURCE_CONFIGS[kind]
            const active = kind === marketTab
            return (
              <button
                key={kind}
                type="button"
                onClick={() => setMarketTab(kind)}
                className={`flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm transition-colors duration-150 ${
                  active
                    ? 'bg-accent-soft font-medium text-accent'
                    : 'text-neutral-500 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800'
                }`}
              >
                <c.icon className="size-4" />
                {c.title}
              </button>
            )
          })}
        </div>
        <div className="ml-auto flex items-center gap-2">
          <span className="text-xs text-neutral-500 dark:text-neutral-400">安装到</span>
          <select
            className="max-w-52 truncate rounded-lg border border-neutral-200 bg-transparent px-2.5 py-1.5 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
            value={marketInstanceId ?? ''}
            onChange={(e) => setMarketInstance(e.target.value || null)}
            aria-label="选择安装目标实例"
          >
            {instances === null && <option value="">加载中…</option>}
            {instances?.length === 0 && <option value="">暂无实例</option>}
            {instances?.map((i) => (
              <option key={i.id} value={i.id}>
                {i.name}
              </option>
            ))}
          </select>
        </div>
      </div>
      <p className="px-6 pt-2 text-xs text-neutral-400 dark:text-neutral-500">
        {cfg.notice ??
          `浏览与安装${cfg.title}到选中的实例；已安装的${cfg.title}请到对应实例的「更多 → 资源管理」管理`}
      </p>

      <main className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6 pt-4">
        {/* key 随类型变化重挂，触发切换淡入 */}
        <MarketPanel
          key={marketTab}
          kind={marketTab}
          instanceId={marketInstanceId}
          instance={instance}
        />
      </main>
    </div>
  )
}
