// 实例列表项：实例页左列的紧凑行（图标 + 名称 + 版本徽章 + 运行状态）
// 单击选中，双击直接启动

import type { InstanceSummary } from '../../types/domain'
import { isBusy, useLaunchStore } from '../../stores/launchStore'

const LOADER_BADGE: Record<string, string> = {
  forge: 'bg-orange-100 text-orange-700 dark:bg-orange-500/15 dark:text-orange-300',
  fabric: 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300',
  neoforge: 'bg-red-100 text-red-700 dark:bg-red-500/15 dark:text-red-300',
  quilt: 'bg-violet-100 text-violet-700 dark:bg-violet-500/15 dark:text-violet-300',
  optifine: 'bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300',
}

interface InstanceListItemProps {
  instance: InstanceSummary
  selected: boolean
  onSelect: (inst: InstanceSummary) => void
  onDoubleClick: (inst: InstanceSummary) => void
}

export function InstanceListItem({ instance, selected, onSelect, onDoubleClick }: InstanceListItemProps) {
  const launchEntry = useLaunchStore((s) => s.entries[instance.id])
  const launchBusy = isBusy(launchEntry)
  // 运行状态色点：启动中橙 / 运行中绿
  const dotCls =
    launchEntry === undefined
      ? 'bg-neutral-300 dark:bg-neutral-600'
      : launchEntry.stage === 'running'
        ? 'bg-emerald-500'
        : 'bg-amber-500 animate-pulse'

  return (
    <button
      type="button"
      onClick={() => onSelect(instance)}
      onDoubleClick={() => onDoubleClick(instance)}
      className={`flex w-full items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors duration-150 ${
        selected
          ? 'bg-accent-soft ring-1 ring-accent/40'
          : 'hover:bg-neutral-100 dark:hover:bg-neutral-800'
      }`}
    >
      <span className={`size-2 shrink-0 rounded-full ${dotCls}`} title={launchBusy ? '运行中' : '未运行'} />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium">{instance.name}</span>
        <span className="mt-0.5 flex items-center gap-1">
          <span className="rounded bg-neutral-100 px-1 py-0.5 text-[10px] text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400">
            {instance.mcVersion}
          </span>
          {LOADER_BADGE[instance.loaderType] && (
            <span className={`rounded px-1 py-0.5 text-[10px] ${LOADER_BADGE[instance.loaderType]}`}>
              {instance.loaderType === 'vanilla' ? '原版' : instance.loaderType}
            </span>
          )}
        </span>
      </span>
    </button>
  )
}
