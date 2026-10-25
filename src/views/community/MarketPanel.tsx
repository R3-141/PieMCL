// 社区资源市场（Modrinth；CurseForge 仅模组）：来源切换 / 类型筛选 / 排序 / 搜索 / 两段式安装
// 进入即展示当前环境下的热门资源（按下载量排序，无需输入关键词）；
// 搜索与下载全部经 Rust 侧，前端不直接请求第三方 API；
// MC 版本与加载器由实例决定（Rust 侧读取），保证与目标实例一致；
// 类型差异（来源 / 分类 / 兼容判断）由 ResourceConfig 驱动。

import { useCallback, useEffect, useRef, useState } from 'react'
import { ipc } from '../../lib/ipc'
import { formatBytes } from '../../lib/format'
import type { InstallPlan, InstanceSummary, OnlineVersion, SearchHit } from '../../types/domain'
import {
  CATEGORIES_BY_KIND,
  CATEGORY_LABELS,
  RESOURCE_CONFIGS,
  type ResourceKind,
} from './config'

const SORTS = [
  { value: 'downloads', label: '最多下载' },
  { value: 'relevance', label: '相关度' },
  { value: 'updated', label: '最近更新' },
  { value: 'newest', label: '最新发布' },
]

const ORIGINS = [
  { value: 'modrinth', label: 'Modrinth' },
  { value: 'curseforge', label: 'CurseForge' },
]

/** 加载器类目徽章（display_categories 中的加载器取值） */
const LOADER_LABEL: Record<string, string> = {
  fabric: 'Fabric',
  forge: 'Forge',
  neoforge: 'NeoForge',
  quilt: 'Quilt',
  iris: 'Iris',
  optifine: 'OptiFine',
  canvas: 'Canvas',
  datapack: '数据包',
  minecraft: '原版',
}

function formatDownloads(n: number): string {
  if (n >= 100_000_000) return `${(n / 100_000_000).toFixed(2)} 亿`
  if (n >= 10_000) return `${(n / 10_000).toFixed(1)} 万`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

/** ISO 时间 → 相对时间（仿应用商店的"X 天前"） */
function relTime(iso: string | null): string {
  if (!iso) return '未知'
  const t = Date.parse(iso)
  if (Number.isNaN(t)) return '未知'
  const diff = Date.now() - t
  const min = Math.floor(diff / 60_000)
  if (min < 1) return '刚刚'
  if (min < 60) return `${min} 分钟前`
  const hour = Math.floor(min / 60)
  if (hour < 24) return `${hour} 小时前`
  const day = Math.floor(hour / 24)
  if (day < 30) return `${day} 天前`
  const month = Math.floor(day / 30)
  if (month < 12) return `${month} 个月前`
  return `${Math.floor(month / 12)} 年前`
}

/** PlanDep → 统一安装目标（mr:... / cf:...） */
function toTarget(d: InstallPlan['main']): string {
  return d.origin === 'curseforge' ? `cf:${d.projectId}:${d.versionId}` : `mr:${d.versionId}`
}

/** 资源图标：经 Rust 代理下载（磁盘缓存），失败显示占位块 */
function ResourceIcon({ url, emoji }: { url: string | null; emoji: string }) {
  const [src, setSrc] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  useEffect(() => {
    if (!url) {
      setFailed(true)
      return
    }
    let alive = true
    setFailed(false)
    setSrc(null)
    ipc
      .fetchIcon(url)
      .then((d) => {
        if (alive) setSrc(d)
      })
      .catch(() => {
        if (alive) setFailed(true)
      })
    return () => {
      alive = false
    }
  }, [url])

  if (src) {
    return <img src={src} alt="" className="size-12 shrink-0 rounded-lg object-cover" />
  }
  return (
    <div
      className={`flex size-12 shrink-0 items-center justify-center rounded-lg bg-neutral-100 text-lg dark:bg-neutral-800 ${
        failed ? '' : 'animate-pulse'
      }`}
    >
      {emoji}
    </div>
  )
}

export function MarketPanel({
  kind,
  instanceId,
  instance,
}: {
  kind: ResourceKind
  instanceId: string | null
  instance: InstanceSummary | null
}) {
  const cfg = RESOURCE_CONFIGS[kind]
  const categories = CATEGORIES_BY_KIND[kind]

  const [query, setQuery] = useState('')
  const [origin, setOrigin] = useState('modrinth')
  const [category, setCategory] = useState('')
  const [sort, setSort] = useState('downloads')
  const [hits, setHits] = useState<SearchHit[] | null>(null)
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [expandedOrigin, setExpandedOrigin] = useState<string | null>(null)
  const [versions, setVersions] = useState<OnlineVersion[] | null>(null)
  const [versionsLoading, setVersionsLoading] = useState(false)
  const [plan, setPlan] = useState<InstallPlan | null>(null)
  const [planLoading, setPlanLoading] = useState(false)
  const [depChecked, setDepChecked] = useState<Record<string, boolean>>({})
  // 推荐安装（optional 依赖）：默认不勾选，由用户决定
  const [recChecked, setRecChecked] = useState<Record<string, boolean>>({})
  const [installDone, setInstallDone] = useState<string | null>(null)

  const mcVersion = instance?.mcVersion ?? ''
  const loader = instance?.loaderType ?? 'vanilla'
  // 原版实例：模组市场整体不可用（需加载器），其余类型可用
  const vanillaBlocked = cfg.supportsCurseforge && loader === 'vanilla'

  // 请求序号防竞态（快速切换筛选时旧响应不覆盖新响应）
  const reqSeq = useRef(0)
  // 版本列表请求序号（手快连续展开不同资源时，旧响应不得覆盖新展开项）
  const versionsSeq = useRef(0)

  const search = useCallback(
    async (offset: number) => {
      if (!instanceId) return
      const seq = ++reqSeq.current
      setLoading(true)
      setError(null)
      try {
        const r = await ipc.searchMods(
          instanceId,
          cfg.resourceType,
          origin,
          query,
          category,
          sort,
          offset,
        )
        if (seq !== reqSeq.current) return
        setHits(offset === 0 ? r.hits : [...(hits ?? []), ...r.hits])
        setTotal(r.total)
      } catch (e) {
        if (seq === reqSeq.current) setError(String(e))
      } finally {
        if (seq === reqSeq.current) setLoading(false)
      }
    },
    [instanceId, cfg.resourceType, origin, query, category, sort, hits],
  )

  // 进入市场 / 切换实例 / 切换类型 Tab / 切换来源 / 切换类型筛选 / 切换排序 → 自动加载
  useEffect(() => {
    setHits(null)
    setExpandedId(null)
    setVersions(null)
    void search(0)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId, cfg.resourceType, origin, category, sort])

  const toggleExpand = async (hit: SearchHit) => {
    if (expandedId === hit.projectId) {
      setExpandedId(null)
      setExpandedOrigin(null)
      setVersions(null)
      return
    }
    const seq = ++versionsSeq.current
    setExpandedId(hit.projectId)
    setExpandedOrigin(hit.origin)
    setVersions(null)
    setVersionsLoading(true)
    try {
      const vs = await ipc.listOnlineVersions(instanceId!, cfg.resourceType, hit.origin, hit.projectId)
      if (seq !== versionsSeq.current) return // 已切换到其他资源，丢弃旧响应
      setVersions(vs)
    } catch (e) {
      if (seq !== versionsSeq.current) return
      setError(String(e))
      setExpandedId(null)
    } finally {
      if (seq === versionsSeq.current) setVersionsLoading(false)
    }
  }

  const startInstall = async (hitOrigin: string, versionId: string) => {
    setPlanLoading(true)
    setError(null)
    try {
      const p = await ipc.planModInstall(instanceId!, cfg.resourceType, hitOrigin, versionId)
      setPlan(p)
      // 必需前置默认全勾；推荐安装默认不勾
      setDepChecked(Object.fromEntries(p.deps.map((d) => [d.versionId, true])))
      setRecChecked(Object.fromEntries(p.recommended.map((d) => [d.versionId, false])))
    } catch (e) {
      setError(String(e))
    } finally {
      setPlanLoading(false)
    }
  }

  const confirmInstall = () => {
    if (!plan) return
    const p = plan
    const checkedDeps = p.deps.filter((d) => depChecked[d.versionId])
    const checkedRecs = p.recommended.filter((d) => recChecked[d.versionId])
    // 立即关闭弹窗，下载转后台（进度见下载面板），完成后顶部提示
    setPlan(null)
    setInstallDone(
      `「${p.main.title}」已开始下载（${1 + checkedDeps.length + checkedRecs.length} 个文件）…`,
    )
    void (async () => {
      try {
        const ids = [toTarget(p.main), ...checkedDeps.map(toTarget), ...checkedRecs.map(toTarget)]
        const names = await ipc.confirmModInstall(instanceId!, cfg.resourceType, ids)
        setInstallDone(`已安装 ${names.length} 个文件到「${instance?.name}」`)
      } catch (e) {
        setError(String(e))
      }
    })()
  }

  /** 内容分类徽章（剔除加载器类目，单独用加载器徽章展示） */
  const categoryBadges = (h: SearchHit): { key: string; label: string; isLoader: boolean }[] =>
    h.displayCategories.slice(0, 4).map((c) => ({
      key: c,
      label: LOADER_LABEL[c.toLowerCase()] ?? CATEGORY_LABELS[c.toLowerCase()] ?? c,
      isLoader: c.toLowerCase() in LOADER_LABEL,
    }))

  const resetFilters = () => {
    setQuery('')
    setCategory('')
    setSort('downloads')
  }

  /** 模组校验加载器兼容；数据包 / 光影 / 资源包只看 MC 版本 */
  const isCompatible = (v: OnlineVersion) =>
    v.gameVersions.includes(mcVersion) &&
    (!cfg.supportsCurseforge || v.loaders.some((l) => l.toLowerCase() === loader))

  const inputCls =
    'rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'

  return (
    <div className="flex h-full flex-col animate-[page-in_150ms_ease-out]">
      {/* 搜索条件区（仿 PCL：来源 / 类型 / 排序 / 关键词 / 重置） */}
      <div className="flex flex-wrap items-center gap-2">
        {cfg.supportsCurseforge ? (
          <div className="flex rounded-lg bg-neutral-100 p-0.5 dark:bg-neutral-800">
            {ORIGINS.map((o) => (
              <button
                key={o.value}
                type="button"
                onClick={() => {
                  setOrigin(o.value)
                  setCategory('')
                }}
                className={`rounded-md px-2.5 py-1.5 text-xs transition-colors duration-150 ${
                  origin === o.value
                    ? 'bg-white font-medium text-neutral-800 shadow-sm dark:bg-neutral-700 dark:text-neutral-100'
                    : 'text-neutral-500 hover:text-neutral-700 dark:text-neutral-400 dark:hover:text-neutral-200'
                }`}
              >
                {o.label}
              </button>
            ))}
          </div>
        ) : (
          <span className="rounded-md bg-neutral-100 px-2.5 py-1.5 text-xs text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400">
            Modrinth
          </span>
        )}
        <select
          className={inputCls}
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          disabled={!instanceId || origin === 'curseforge'}
          title={origin === 'curseforge' ? 'CurseForge 源暂不支持类型筛选' : undefined}
          aria-label="类型筛选"
        >
          {categories.map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </select>
        <select
          className={inputCls}
          value={sort}
          onChange={(e) => setSort(e.target.value)}
          aria-label="排序"
        >
          {SORTS.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </select>
        <input
          className={`${inputCls} min-w-0 flex-1`}
          placeholder={`按名称搜索（留空 = 浏览热门${cfg.title}）`}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void search(0)
          }}
          disabled={!instanceId}
        />
        <button
          type="button"
          onClick={() => void search(0)}
          disabled={loading || !instanceId}
          className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
        >
          搜索
        </button>
        <button
          type="button"
          onClick={resetFilters}
          disabled={loading || !instanceId}
          className="rounded-lg border border-neutral-200 px-3 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
        >
          重置
        </button>
      </div>
      <p className="mt-1.5 text-xs text-neutral-400 dark:text-neutral-500">
        环境筛选：MC {mcVersion || '?'}
        {loader !== 'vanilla' ? ` · ${LOADER_LABEL[loader] ?? loader}` : ` · ${cfg.vanillaMarketNote}`}
        {instance && ` · 安装目标「${instance.name}」`}
        {origin === 'curseforge' && ' · CurseForge 源暂不支持类型筛选'}
      </p>

      {vanillaBlocked && (
        <div className="mt-3 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700 dark:bg-amber-500/10 dark:text-amber-300">
          当前实例为原版，模组需要 Fabric / Forge 等加载器才能安装。可先在实例「更多」中安装加载器。
        </div>
      )}
      {installDone && (
        <div className="mt-3 rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300">
          {installDone}
        </div>
      )}
      {error && (
        <div className="mt-3 break-all rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
          {error}
        </div>
      )}

      {/* 结果列表 */}
      <div className="mt-3 min-h-0 flex-1 overflow-y-auto pr-1">
        {hits === null ? (
          <p className="py-10 text-center text-sm text-neutral-400 dark:text-neutral-500">
            {loading ? `正在加载热门${cfg.title}…` : '加载中…'}
          </p>
        ) : (
          <>
            <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
              {hits.map((h) => (
                <div
                  key={h.projectId}
                  className="rounded-xl border border-neutral-200 bg-white p-3 transition-colors duration-150 hover:border-accent/50 dark:border-neutral-800 dark:bg-neutral-900"
                >
                  <button
                    type="button"
                    className="flex w-full items-start gap-3 text-left"
                    onClick={() => void toggleExpand(h)}
                  >
                    <ResourceIcon url={h.iconUrl} emoji="🧩" />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-baseline gap-2">
                        <span className="truncate text-sm font-medium">{h.title}</span>
                        <span className="shrink-0 text-xs text-neutral-400 dark:text-neutral-500">
                          {h.author}
                        </span>
                      </div>
                      {/* 分类徽章 */}
                      <div className="mt-1 flex flex-wrap gap-1">
                        {categoryBadges(h).map((b) => (
                          <span
                            key={b.key}
                            className={`rounded px-1.5 py-0.5 text-[10px] ${
                              b.isLoader
                                ? 'bg-accent-soft text-accent'
                                : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
                            }`}
                          >
                            {b.label}
                          </span>
                        ))}
                      </div>
                      <p className="mt-1 line-clamp-1 text-xs text-neutral-500 dark:text-neutral-400">
                        {h.description}
                      </p>
                      {/* 元信息行：下载量 · 更新时间 · 来源 */}
                      <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-neutral-400 dark:text-neutral-500">
                        <span>⬇ {formatDownloads(h.downloads)}</span>
                        <span>⟳ {relTime(h.dateModified)}</span>
                        <span>{h.origin === 'curseforge' ? 'CurseForge' : 'Modrinth'}</span>
                      </div>
                    </div>
                  </button>

                  {/* 展开的版本列表 */}
                  {expandedId === h.projectId && (
                    <div className="mt-3 max-h-56 space-y-1.5 overflow-y-auto border-t border-neutral-100 pt-3 animate-[page-in_160ms_ease-out] dark:border-neutral-800">
                      {versionsLoading ? (
                        <p className="py-2 text-center text-xs text-neutral-400">加载版本中…</p>
                      ) : versions && versions.length > 0 ? (
                        versions.map((v) => {
                          const compat = isCompatible(v)
                          return (
                            <div
                              key={v.id}
                              className={`flex animate-[item-in_140ms_ease-out] items-center gap-2 rounded-lg px-2 py-1.5 text-xs ${
                                compat
                                  ? 'bg-neutral-50 dark:bg-neutral-800/60'
                                  : 'bg-neutral-50 opacity-60 dark:bg-neutral-800/40'
                              }`}
                            >
                              <span className="font-mono">{v.versionNumber}</span>
                              <span className="rounded bg-neutral-200 px-1 py-0.5 text-[10px] text-neutral-600 dark:bg-neutral-700 dark:text-neutral-300">
                                {v.versionType}
                              </span>
                              {!compat && (
                                <span className="text-[10px] text-red-400">不兼容当前实例</span>
                              )}
                              <span className="ml-auto text-neutral-400">
                                {formatBytes(v.files[0]?.size ?? 0)}
                              </span>
                              <button
                                type="button"
                                onClick={() => void startInstall(expandedOrigin ?? origin, v.id)}
                                disabled={planLoading || !compat}
                                className="shrink-0 rounded-md bg-accent-soft px-2.5 py-1 font-medium text-accent transition-colors duration-150 hover:bg-accent hover:text-white disabled:cursor-not-allowed disabled:opacity-40"
                              >
                                安装
                              </button>
                            </div>
                          )
                        })
                      ) : (
                        <p className="py-2 text-center text-xs text-neutral-400">没有可用版本</p>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
            {hits.length === 0 && (
              <p className="py-10 text-center text-sm text-neutral-400 dark:text-neutral-500">
                {query
                  ? `没有匹配「${query}」的结果`
                  : `当前环境下暂无热门${cfg.title}（或该分类下没有兼容内容）`}
              </p>
            )}
            {hits.length < total && (
              <div className="py-4 text-center">
                <button
                  type="button"
                  onClick={() => void search(hits.length)}
                  disabled={loading}
                  className="rounded-lg bg-neutral-100 px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
                >
                  {loading ? '加载中…' : `加载更多（共 ${total} 个）`}
                </button>
              </div>
            )}
          </>
        )}
      </div>

      {/* 安装确认弹窗（依赖树） */}
      {plan && (
        <div className="absolute inset-0 z-50 flex animate-[fade-in_150ms_ease-out] items-center justify-center bg-black/40 p-6">
          <div className="max-h-[80%] w-full max-w-lg animate-[modal-in_180ms_ease-out] overflow-y-auto rounded-2xl bg-white p-5 shadow-2xl dark:bg-neutral-900">
            <h3 className="text-base font-semibold">确认安装</h3>
            <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
              将安装到实例：
              <span className="font-medium text-accent">{instance?.name}</span>
              （MC {mcVersion}
              {loader !== 'vanilla' ? ` · ${LOADER_LABEL[loader] ?? loader}` : ''}）
            </p>

            {/* 主文件 */}
            <div className="mt-3 rounded-lg bg-neutral-50 px-3 py-2 dark:bg-neutral-800/60">
              <div className="flex items-center gap-2 text-sm">
                <span className="font-medium">{plan.main.title}</span>
                <span className="font-mono text-xs text-neutral-500">
                  {plan.main.versionNumber}
                </span>
              </div>
              <p className="mt-0.5 truncate font-mono text-xs text-neutral-400">
                {plan.main.file.filename} · {formatBytes(plan.main.file.size)}
              </p>
            </div>

            {/* 依赖树 */}
            {plan.deps.length > 0 && (
              <div className="mt-3">
                <p className="text-xs font-medium text-neutral-600 dark:text-neutral-300">
                  需要的前置依赖（{plan.deps.length}）
                </p>
                <div className="mt-1.5 space-y-1">
                  {plan.deps.map((d) => (
                    <label
                      key={d.versionId}
                      className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-50 dark:hover:bg-neutral-800"
                    >
                      <input
                        type="checkbox"
                        checked={depChecked[d.versionId] ?? false}
                        onChange={(e) =>
                          setDepChecked((m) => ({ ...m, [d.versionId]: e.target.checked }))
                        }
                        className="size-4 accent-[var(--accent)]"
                      />
                      <span className="min-w-0 flex-1 truncate">
                        {d.title}
                        <span className="ml-1.5 font-mono text-xs text-neutral-400">
                          {d.versionNumber}
                        </span>
                      </span>
                      <span className="shrink-0 text-xs text-neutral-400">
                        {formatBytes(d.file.size)}
                      </span>
                    </label>
                  ))}
                </div>
              </div>
            )}
            {plan.recommended.length > 0 && (
              <div className="mt-3">
                <p className="text-xs font-medium text-neutral-600 dark:text-neutral-300">
                  推荐安装（{plan.recommended.length}，作者标记为可选）
                </p>
                <div className="mt-1.5 space-y-1">
                  {plan.recommended.map((d) => (
                    <label
                      key={d.versionId}
                      className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-50 dark:hover:bg-neutral-800"
                    >
                      <input
                        type="checkbox"
                        checked={recChecked[d.versionId] ?? false}
                        onChange={(e) =>
                          setRecChecked((m) => ({ ...m, [d.versionId]: e.target.checked }))
                        }
                        className="size-4 accent-[var(--accent)]"
                      />
                      <span className="min-w-0 flex-1 truncate">
                        {d.title}
                        <span className="ml-1.5 font-mono text-xs text-neutral-400">
                          {d.versionNumber}
                        </span>
                      </span>
                      <span className="shrink-0 text-xs text-neutral-400">
                        {formatBytes(d.file.size)}
                      </span>
                    </label>
                  ))}
                </div>
              </div>
            )}
            {plan.unknownDeps.length > 0 && (
              <p className="mt-3 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700 dark:bg-amber-500/10 dark:text-amber-300">
                以下前置无法自动定位版本，可能需要手动处理：{plan.unknownDeps.join('、')}
              </p>
            )}

            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setPlan(null)}
                className="rounded-lg bg-neutral-100 px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
              >
                取消
              </button>
              <button
                type="button"
                onClick={confirmInstall}
                className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
              >
                安装（{1 + plan.deps.filter((d) => depChecked[d.versionId]).length} 个文件）
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
