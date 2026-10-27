// 首页（启动器默认页）：左侧账户卡 + 中间留白 + 右下角启动按钮组
// 账户卡参照 PCL2：3D 头像 + 用户名、账户类型行（Microsoft 四色 Logo）、皮肤预览（可刷新）+ 3D 模型
// 启动按钮主区为「启动」大字 + 实例名小字，右侧箭头向上弹出实例选择抽屉

import { useCallback, useEffect, useMemo, useState } from 'react'
import { ipc, onAccountsChanged, onInstancesChanged } from '../../lib/ipc'
import { useUiStore } from '../../stores/uiStore'
import { useLaunchStore, isBusy, STAGE_LABEL } from '../../stores/launchStore'
import type { AccountListResult, AccountView, InstanceSummary } from '../../types/domain'
import { SkinCanvas } from '../player/SkinCanvas'
import { IconMore } from '../../components/common/Icons'

/** 贴图 URL → data URL（经 Rust 代理，绕开 http 混合内容与直连失败） */
function useTextureDataUrl(
  url: string | null | undefined,
  refreshKey: number,
): string | null {
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  useEffect(() => {
    if (!url) {
      setDataUrl(null)
      return
    }
    let alive = true
    setDataUrl(null)
    ipc
      .fetchIcon(url)
      .then((d) => {
        if (alive) setDataUrl(d)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [url, refreshKey])
  return dataUrl
}

/** 皮肤贴图裁出头像：底层脸部 (8,8)-(16,16) + 帽层脸部 (40,8)-(48,16) 叠加（第二层浮起观感） */
function FaceCrop({ tex, size }: { tex: string | null; size: number }) {
  // 贴图放大 8 倍平铺，区域起点 (x,y) 的偏移 = -(x/64)*8*size → x=8 为 -100%，x=40 为 -500%
  return (
    <div
      className="relative shrink-0 overflow-hidden rounded-lg bg-neutral-200 dark:bg-neutral-800"
      style={{ width: size, height: size }}
    >
      {tex && (
        <>
          <img
            src={tex}
            alt=""
            draggable={false}
            className="absolute max-w-none"
            style={{
              width: '800%',
              height: '800%',
              left: '-100%',
              top: '-100%',
              imageRendering: 'pixelated',
            }}
          />
          <img
            src={tex}
            alt=""
            draggable={false}
            className="absolute max-w-none"
            style={{
              width: '800%',
              height: '800%',
              left: '-500%',
              top: '-100%',
              imageRendering: 'pixelated',
            }}
          />
        </>
      )}
    </div>
  )
}

/** Microsoft 四色 Logo（账户类型行） */
function MsLogo() {
  return (
    <div className="grid size-5 shrink-0 grid-cols-2 gap-[2px]">
      <span className="rounded-[1px] bg-[#F25022]" />
      <span className="rounded-[1px] bg-[#7FBA00]" />
      <span className="rounded-[1px] bg-[#00A4EF]" />
      <span className="rounded-[1px] bg-[#FFB900]" />
    </div>
  )
}

/** 离线账户图标（灰色人形） */
function OfflineLogo() {
  return (
    <div className="flex size-5 shrink-0 items-center justify-center rounded bg-neutral-300 dark:bg-neutral-600">
      <svg viewBox="0 0 16 16" fill="currentColor" className="size-3 text-white">
        <path d="M8 8a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm-5.5 6a5.5 5.5 0 0 1 11 0h-11Z" />
      </svg>
    </div>
  )
}

export function HomeView() {
  const setCurrentView = useUiStore((s) => s.setCurrentView)
  const setSelectedInstance = useUiStore((s) => s.setSelectedInstance)
  const launchEntries = useLaunchStore((s) => s.entries)

  const [accounts, setAccounts] = useState<AccountListResult | null>(null)
  const [instances, setInstances] = useState<InstanceSummary[]>([])
  const [drawerOpen, setDrawerOpen] = useState(false)
  const [launching, setLaunching] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // 抽屉临时选择：仅影响下一次启动目标，启动后清空回退「最近启动」
  const [drawerPick, setDrawerPick] = useState<string | null>(null)
  // 皮肤预览刷新计数（重挂载 3D 画布 + 重拉贴图）
  const [refreshKey, setRefreshKey] = useState(0)

  const refreshAccounts = useCallback(() => {
    ipc.listAccounts().then(setAccounts).catch(() => {})
  }, [])
  const refreshInstances = useCallback(() => {
    ipc.listInstances().then(setInstances).catch(() => {})
  }, [])

  // 初始加载 + 增删改事件刷新
  useEffect(() => {
    refreshAccounts()
    refreshInstances()
    const offs: Array<() => void> = []
    onAccountsChanged(refreshAccounts).then((u) => offs.push(u))
    onInstancesChanged(refreshInstances).then((u) => offs.push(u))
    // 启动器重启后 launchStore 为空，同步一次仍在运行的进程
    ipc
      .listRunning()
      .then((running) => {
        for (const r of running) {
          useLaunchStore.getState().applyState({
            instanceId: r.instanceId,
            stage: 'running',
            message: '运行中',
          })
        }
      })
      .catch(() => {})
    return () => offs.forEach((u) => u())
  }, [refreshAccounts, refreshInstances])

  const active: AccountView | null =
    accounts?.accounts.find((a) => a.id === accounts?.activeId) ?? null

  // 账户卡皮肤贴图（3D 头像与身体预览共用）
  const activeSkin = active?.skins.find((s) => s.state === 'ACTIVE') ?? null
  // 正版账户当前装备的披风（离线账户无披风数据，预览不提供显隐选项）
  const activeCape =
    active?.kind === 'microsoft'
      ? (active.capes.find((c) => c.id === active.activeCapeId) ?? null)
      : null
  const [showCape, setShowCape] = useState(true)
  const capeTex = useTextureDataUrl(activeCape?.url ?? null, refreshKey)
  const [offlineSkinTex, setOfflineSkinTex] = useState<string | null>(null)
  const msSkinTex = useTextureDataUrl(
    active?.kind === 'microsoft' ? activeSkin?.url : null,
    refreshKey,
  )
  useEffect(() => {
    let alive = true
    setOfflineSkinTex(null)
    if (active?.kind === 'offline') {
      ipc
        .getOfflineSkinDataUrl(active.id)
        .then((d) => {
          if (alive) setOfflineSkinTex(d)
        })
        .catch(() => {})
    }
    return () => {
      alive = false
    }
  }, [active?.id, active?.kind, active?.offlineSkin?.fetchedAt, refreshKey])
  // 离线账户未设置皮肤时，按玩家名拉默认/对应正版皮肤（mc-heads），避免 3D 预览空白
  const offlineFallback = useTextureDataUrl(
    active?.kind === 'offline' && !active.offlineSkin
      ? `https://mc-heads.net/skin/${encodeURIComponent(active.name)}`
      : null,
    refreshKey,
  )
  const skinTex =
    active?.kind === 'microsoft' ? msSkinTex : (offlineSkinTex ?? offlineFallback)

  // 正版账户头像：NMSR 3D 面部渲染（含第二层立体透视），离线账户回退平铺 FaceCrop。
  // ?t= 使刷新按钮能绕过 fetch_icon 缓存重新拉取渲染
  const nmsrAvatarTex = useTextureDataUrl(
    active?.kind === 'microsoft'
      ? `https://nmsr.nickac.dev/face/${active.uuid}?t=${refreshKey}`
      : null,
    refreshKey,
  )

  // 默认启动目标：抽屉选择 > 最近启动 > 列表第一个。
  // 注意不读全局 selectedInstanceId——那会跟着实例页的浏览选中走，
  // 导致首页按钮不跟随最近启动的实例
  const target = useMemo(() => {
    if (instances.length === 0) return null
    if (drawerPick) {
      const picked = instances.find((i) => i.id === drawerPick)
      if (picked) return picked
    }
    return [...instances].sort((a, b) => (b.lastLaunchedAt ?? 0) - (a.lastLaunchedAt ?? 0))[0]
  }, [instances, drawerPick])

  const entry = target ? launchEntries[target.id] : undefined
  const busy = isBusy(entry)
  const busyLabel =
    entry?.stage === 'running' ? '游戏运行中' : entry ? (STAGE_LABEL[entry.stage] ?? '运行中') : ''

  const handleLaunch = async () => {
    if (!target) return
    setError(null)
    // 运行中时点击跳转日志页查看
    if (busy) {
      useLaunchStore.getState().setActive(target.id)
      setCurrentView('log')
      return
    }
    setLaunching(true)
    try {
      await ipc.launchInstance(target.id)
      // 启动受理成功：清抽屉选择（回退「最近启动」）并刷新实例列表，
      // 让 lastLaunchedAt 立即生效——下次默认目标就是刚启动的这个
      setDrawerPick(null)
      refreshInstances()
    } catch (e) {
      setError(String(e))
    } finally {
      setLaunching(false)
    }
  }

  const gotoPlayer = () => setCurrentView('player')
  /** 三个点：直达当前账户的管理详情页（换皮肤 / 披风） */
  const gotoAccountDetail = () => {
    if (active) useUiStore.getState().setPendingAccountDetail(active.id)
    setCurrentView('player')
  }

  const cardBg = 'bg-white dark:bg-neutral-900'
  const borderColor = 'border-neutral-200 dark:border-neutral-800'

  return (
    <div className="relative h-full min-h-[480px]">
      {/* 左侧账户面板：上下贴合内容区、无卡片框，仅右侧一条分隔线 */}
      <aside
        className={`absolute inset-y-0 left-0 flex w-80 flex-col border-r ${borderColor} bg-white/50 dark:bg-black/25`}
      >
        {active ? (
          <>
            {/* 头像（正版 = NMSR 3D 面部渲染，离线 = 平铺脸部；点击进账户管理） */}
            <div className="flex items-center gap-3 px-5 pt-5">
              <button
                type="button"
                onClick={gotoPlayer}
                title="进入账户管理"
                className="rounded-lg transition-opacity duration-150 hover:opacity-80"
              >
                {nmsrAvatarTex ? (
                  <img
                    src={nmsrAvatarTex}
                    alt={active.name}
                    draggable={false}
                    className="size-14 [image-rendering:pixelated]"
                  />
                ) : (
                  <FaceCrop tex={skinTex} size={56} />
                )}
              </button>
              <div className="min-w-0 flex-1">
                <p className="truncate text-base font-semibold">{active.name}</p>
                <p className="mt-0.5 flex items-center gap-1.5 text-xs text-neutral-500 dark:text-neutral-400">
                  <span
                    className={`size-1.5 rounded-full ${
                      active.kind === 'microsoft' ? 'bg-emerald-500' : 'bg-amber-500'
                    }`}
                  />
                  {active.kind === 'microsoft' ? '已登录' : '离线模式'}
                </p>
              </div>
              {/* 更多：直达账户管理详情页（换皮肤 / 披风） */}
              <button
                type="button"
                title="管理账户"
                onClick={gotoAccountDetail}
                className="shrink-0 rounded-md p-1.5 text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-white/10 dark:hover:text-neutral-300"
              >
                <IconMore className="size-4" />
              </button>
            </div>

            {/* 账户类型行（整行点击进账户管理） */}
            <button
              type="button"
              onClick={gotoPlayer}
              className="mt-4 flex w-full items-center gap-2.5 px-5 text-left"
            >
              {active.kind === 'microsoft' ? <MsLogo /> : <OfflineLogo />}
              <span className="min-w-0 flex-1 truncate text-sm font-medium">
                {active.kind === 'microsoft' ? 'Microsoft 正版账户' : '离线账户'}
              </span>
              <span aria-hidden className="text-sm text-neutral-400">
                ›
              </span>
            </button>

            {/* 皮肤预览：占满面板剩余高度 */}
            <div className="mt-5 flex min-h-0 flex-1 flex-col px-5 pb-5">
              <div className="flex items-center justify-between">
                <p className="text-sm font-semibold">皮肤预览</p>
                <div className="flex items-center gap-1">
                  {/* 披风显隐：仅正版账户装备了披风时出现，默认显示 */}
                  {activeCape && (
                    <button
                      type="button"
                      onClick={() => setShowCape((v) => !v)}
                      title={showCape ? '隐藏披风' : '显示披风'}
                      className={`rounded-md p-1 transition-colors duration-150 ${
                        showCape
                          ? 'text-accent'
                          : 'text-neutral-400 hover:text-neutral-600 dark:hover:text-neutral-300'
                      }`}
                    >
                      <svg viewBox="0 0 16 16" fill="none" className="size-3.5">
                        {showCape ? (
                          <>
                            <path
                              d="M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8Z"
                              stroke="currentColor"
                              strokeWidth="1.4"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            />
                            <circle cx="8" cy="8" r="2" stroke="currentColor" strokeWidth="1.4" />
                          </>
                        ) : (
                          <>
                            <path
                              d="M3 3l10 10"
                              stroke="currentColor"
                              strokeWidth="1.4"
                              strokeLinecap="round"
                            />
                            <path
                              d="M5.6 4.1A7.8 7.8 0 0 1 8 3.5c4 0 6.5 4.5 6.5 4.5a12 12 0 0 1-2 2.4M9.8 12.3a7.7 7.7 0 0 1-1.8.2C4 12.5 1.5 8 1.5 8a12.4 12.4 0 0 1 2.3-2.7"
                              stroke="currentColor"
                              strokeWidth="1.4"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            />
                          </>
                        )}
                      </svg>
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => setRefreshKey((k) => k + 1)}
                    title="刷新皮肤"
                    className="rounded-md p-1 text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800 dark:hover:text-neutral-300"
                  >
                    <svg viewBox="0 0 16 16" fill="none" className="size-3.5">
                      <path
                        d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v2.6h-2.6"
                        stroke="currentColor"
                        strokeWidth="1.4"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </button>
                </div>
              </div>
              {/* 轻玻璃底：背景图透过画布（skinview3d 透明渲染）直接可见，避免深色块突兀 */}
              <div className="mt-3 flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-lg border border-black/5 bg-white/20 dark:border-white/10 dark:bg-white/5">
                {skinTex ? (
                  <SkinCanvas
                    key={`${active.id}-${refreshKey}`}
                    skinUrl={skinTex}
                    capeUrl={activeCape ? capeTex : null}
                    showCape={showCape}
                    model={
                      active.kind === 'microsoft'
                        ? (activeSkin?.variant ?? 'classic')
                        : (active.offlineSkin?.variant ?? 'classic')
                    }
                    width={260}
                    height={360}
                  />
                ) : (
                  <p className="text-xs text-neutral-400 dark:text-neutral-500">
                    皮肤贴图加载中…
                  </p>
                )}
              </div>
            </div>
          </>
        ) : (
          /* 未登录引导（面板居中） */
          <div className="flex flex-1 flex-col items-center justify-center px-5 text-center">
            <div className="flex size-12 items-center justify-center rounded-full bg-neutral-100 text-lg dark:bg-neutral-800">
              ?
            </div>
            <p className="mt-3 text-sm font-medium">尚未登录账户</p>
            <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
              添加正版或离线账户后即可启动游戏
            </p>
            <button
              type="button"
              onClick={gotoPlayer}
              className="mt-3 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
            >
              去添加账户 ›
            </button>
          </div>
        )}
      </aside>

      {/* 中间留白 */}

      {/* 启动错误提示 */}
      {error && (
        <p className="absolute bottom-28 right-8 max-w-md truncate rounded-lg bg-red-50 px-3 py-1.5 text-xs text-red-600 dark:bg-red-500/10 dark:text-red-300">
          启动失败：{error}
        </p>
      )}

      {/* 右下角启动按钮组（卡片质感：半透明磨砂，跟随背景卡片样式） */}
      <div className="absolute bottom-8 right-8 z-20 flex items-stretch gap-2">
        <button
          type="button"
          onClick={handleLaunch}
          disabled={!target || launching}
          className={`flex min-w-44 flex-col items-center justify-center rounded-xl border ${borderColor} ${cardBg} px-10 py-4 shadow-lg transition-all duration-150 hover:border-accent active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-50`}
        >
          <span
            key={launching ? 'launching' : busy ? (entry?.stage ?? 'busy') : 'idle'}
            className={`animate-[fade-in_150ms_ease-out] text-2xl font-bold leading-tight ${
              busy ? 'text-emerald-500' : 'text-accent'
            }`}
          >
            {launching ? '启动中…' : busy ? busyLabel : '启动'}
          </span>
          <span className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
            {target ? target.name : '暂无实例'}
          </span>
        </button>
        {/* 向上弹出实例选择抽屉 */}
        <button
          type="button"
          onClick={() => setDrawerOpen((v) => !v)}
          title="选择要启动的实例"
          className={`flex w-12 items-center justify-center rounded-xl border ${borderColor} ${cardBg} text-neutral-500 shadow-sm transition-colors duration-150 hover:bg-neutral-50 dark:text-neutral-400 dark:hover:bg-neutral-800/60 ${
            drawerOpen ? 'text-accent dark:text-accent' : ''
          }`}
        >
          <svg viewBox="0 0 16 16" fill="none" className="size-4">
            <path
              d="M3 10l5-5 5 5"
              stroke="currentColor"
              strokeWidth="1.6"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </button>
      </div>

      {/* 实例选择抽屉（常驻过渡：开合均有淡入/位移；固定定位，避免 Tailwind 定位类失效） */}
      <>
        {/* 点击空白处关闭 */}
        <div
          className={`fixed inset-0 z-30 transition-opacity duration-150 ${
            drawerOpen ? 'opacity-100' : 'pointer-events-none opacity-0'
          }`}
          onClick={() => setDrawerOpen(false)}
        />
        <div
          style={{ position: 'fixed', right: 40, bottom: 92, maxHeight: 320 }}
          className={`z-40 w-72 overflow-y-auto rounded-xl border ${borderColor} ${cardBg} p-1.5 shadow-xl transition-all duration-150 ${
            drawerOpen
              ? 'translate-y-0 opacity-100'
              : 'pointer-events-none translate-y-2 opacity-0'
          }`}
        >
            <p className="px-2.5 pb-1 pt-1.5 text-[10px] text-neutral-400 dark:text-neutral-500">
              选择要启动的实例
            </p>
            {instances.length === 0 ? (
              <p className="px-2.5 pb-2.5 py-2 text-xs text-neutral-400 dark:text-neutral-500">
                还没有实例，请先到「实例」页创建
              </p>
            ) : (
              instances.map((inst) => {
                const running = isBusy(launchEntries[inst.id])
                const selected = target?.id === inst.id
                return (
                  <button
                    key={inst.id}
                    type="button"
                    onClick={() => {
                      setDrawerPick(inst.id)
                      // 同步实例页选中（跨页高亮一致）
                      setSelectedInstance(inst.id)
                      setDrawerOpen(false)
                    }}
                    className={`flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left transition-colors duration-150 ${
                      selected
                        ? 'bg-accent-soft text-accent'
                        : 'hover:bg-neutral-100 dark:hover:bg-neutral-800'
                    }`}
                  >
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-sm font-medium">{inst.name}</span>
                      <span className="block truncate text-[10px] text-neutral-400 dark:text-neutral-500">
                        {inst.mcVersion}
                        {inst.loaderType !== 'vanilla' ? ` · ${inst.loaderType}` : ''}
                      </span>
                    </span>
                    {running && (
                      <span className="size-1.5 shrink-0 rounded-full bg-emerald-500" title="运行中" />
                    )}
                    {selected && <span className="shrink-0 text-xs">✓</span>}
                  </button>
                )
              })
            )}
          </div>
        </>
      </div>
  )
}
