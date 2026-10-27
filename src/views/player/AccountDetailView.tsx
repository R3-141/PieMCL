// 账户详情页：3D 皮肤预览 + 皮肤库 / 上传 / URL 应用 + 披风装备
// 从玩家页账户列表点击进入；数据由父组件传入并随 accounts-changed 自动刷新

import { useCallback, useEffect, useState } from 'react'
import { ipc } from '../../lib/ipc'
import { useUiStore } from '../../stores/uiStore'
import type { AccountView, OfflineSkinMeta } from '../../types/domain'
import { IconPlus } from '../../components/common/Icons'
import { Modal } from '../../components/common/Modal'
import { SkinCanvas } from './SkinCanvas'
import { CapePreview } from './CapePreview'

type Variant = 'slim' | 'classic'

/** 离线皮肤来源的可读标签 */
function offlineSourceLabel(s: OfflineSkinMeta): string {
  switch (s.source) {
    case 'file':
      return `本地文件「${s.sourceName}」`
    case 'url':
      return `URL 皮肤（${s.sourceName}）`
    case 'preset':
      return `预设皮肤 ${s.sourceName}`
    default:
      return `来自玩家「${s.sourceName}」`
  }
}

/** 贴图 URL → data URL（经 Rust 代理，绕开 http 混合内容与直连失败） */
function useTextureDataUrl(url: string | null | undefined): string | null {
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
  }, [url])
  return dataUrl
}

export function AccountDetailView({ account, onBack }: { account: AccountView; onBack: () => void }) {
  const isMicrosoft = account.kind === 'microsoft'
  const isAuthlib = account.kind === 'authlib'
  const activeSkin = account.skins.find((s) => s.state === 'ACTIVE') ?? null
  const activeCape = account.capes.find((c) => c.id === account.activeCapeId) ?? null

  // 皮肤站角色当前皮肤（3D 预览用）
  const [authlibSkin, setAuthlibSkin] = useState<{ dataUrl: string; model: string } | null>(null)
  useEffect(() => {
    let alive = true
    setAuthlibSkin(null)
    if (account.kind === 'authlib') {
      ipc
        .getAuthlibSkinData(account.id)
        .then((d) => {
          if (alive && d) setAuthlibSkin({ dataUrl: d.dataUrl, model: d.model })
        })
        .catch(() => {})
    }
    return () => {
      alive = false
    }
  }, [account.id, account.kind])

  // 3D 预览贴图（data URL）
  const skinTex = useTextureDataUrl(activeSkin?.url)
  const capeTex = useTextureDataUrl(activeCape?.url)
  const [showCape, setShowCape] = useState(true)

  // 离线账户注入皮肤（批次 D）
  const offlineSkin = account.offlineSkin
  const [offlineSkinTex, setOfflineSkinTex] = useState<string | null>(null)
  const [playerNameInput, setPlayerNameInput] = useState('')
  useEffect(() => {
    let alive = true
    setOfflineSkinTex(null)
    if (account.kind === 'offline') {
      ipc
        .getOfflineSkinDataUrl(account.id)
        .then((d) => {
          if (alive) setOfflineSkinTex(d)
        })
        .catch(() => {})
    }
    return () => {
      alive = false
    }
  }, [account.id, account.offlineSkin?.fetchedAt, account.kind])

  const handleFetchOfflineSkin = async () => {
    const name = playerNameInput.trim()
    if (!name) return
    await act(
      'fetch-offline',
      () => ipc.fetchAndApplyOfflineSkin(account.id, name),
      `已抓取「${name}」的皮肤并应用`,
    )
    setPlayerNameInput('')
  }

  const handleRemoveOfflineSkin = async () => {
    if (!window.confirm('移除离线皮肤？游戏内将恢复默认外观。')) return
    await act('remove-offline', () => ipc.removeOfflineSkin(account.id), '已移除离线皮肤')
  }

  // 离线皮肤多来源（预设 / 本地文件 / URL）
  const [offlineUrlInput, setOfflineUrlInput] = useState('')
  const handleOfflineFile = () =>
    act('offline-file', () => ipc.applyOfflineSkinFile(account.id), '已应用本地皮肤文件')
  const handleOfflineUrl = async () => {
    const url = offlineUrlInput.trim()
    if (!url) return
    await act('offline-url', () => ipc.applyOfflineSkinUrl(account.id, url), '已应用 URL 皮肤')
    setOfflineUrlInput('')
  }

  // 暂存选择（预设 + 模型）：点「应用设定」才提交，未选择时跟随当前皮肤
  const [pendingPreset, setPendingPreset] = useState<'steve' | 'alex' | null>(null)
  const [pendingVariant, setPendingVariant] = useState<'classic' | 'slim' | null>(null)
  const currentPreset =
    offlineSkin?.source === 'preset' ? (offlineSkin.sourceName.toLowerCase() as 'steve' | 'alex') : null
  const shownPreset = pendingPreset ?? currentPreset
  const shownVariant =
    pendingVariant ?? ((offlineSkin?.variant as 'classic' | 'slim' | undefined) ?? 'classic')
  const settingsDirty = pendingPreset !== null || pendingVariant !== null
  const handleApplySettings = async () => {
    if (!settingsDirty || busyKey) return
    setBusyKey('apply-settings')
    setError(null)
    setMessage(null)
    try {
      if (pendingPreset && pendingPreset !== currentPreset) {
        await ipc.applyOfflinePreset(account.id, pendingPreset)
      }
      if (pendingVariant) {
        if (!offlineSkin && !pendingPreset) {
          throw new Error('请先设置皮肤（预设 / 本地 / URL / 抓取），再应用模型')
        }
        if (offlineSkin?.variant !== pendingVariant) {
          await ipc.setOfflineSkinVariant(account.id, pendingVariant)
        }
      }
      setMessage('皮肤设定已应用')
      setPendingPreset(null)
      setPendingVariant(null)
    } catch (e) {
      setError(String(e))
    } finally {
      setBusyKey(null)
    }
  }

  // 上传 / URL 应用的模型选择（默认跟随当前皮肤）
  const [variant, setVariant] = useState<Variant>(
    activeSkin?.variant === 'classic' ? 'classic' : 'slim',
  )
  const [urlInput, setUrlInput] = useState('')
  /** 添加皮肤弹窗（拖拽上传 / 文件选择 / URL 应用） */
  const [uploadOpen, setUploadOpen] = useState(false)

  const [busyKey, setBusyKey] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const runAction = useCallback(
    async (key: string, action: () => Promise<void>, okMsg: string, refresh: () => Promise<void>) => {
      if (busyKey) return
      setBusyKey(key)
      setError(null)
      setMessage(null)
      try {
        await action()
        setMessage(okMsg)
        await refresh()
      } catch (e) {
        setError(String(e))
      } finally {
        setBusyKey(null)
      }
    },
    [busyKey],
  )

  const act = (key: string, fn: () => Promise<void>, okMsg: string) =>
    runAction(key, fn, okMsg, async () => {
      /* 父组件经 accounts-changed 自动刷新 */
    })

  const handleUpload = async (path: string) => {
    await act(`upload-${variant}`, () => ipc.uploadSkin(account.id, path, variant), '皮肤已上传')
    setUploadOpen(false)
  }

  /** 打开添加皮肤弹窗：接管全窗拖放（拖入 PNG 直接上传） */
  const openUploadModal = () => {
    const ui = useUiStore.getState()
    ui.setDragMode('skin')
    ui.setSkinDropHandler((pngPath) => void handleUpload(pngPath))
    setUploadOpen(true)
  }

  /** 关闭弹窗：恢复默认拖放分流 */
  const closeUploadModal = () => {
    const ui = useUiStore.getState()
    ui.setDragMode('resource')
    ui.setSkinDropHandler(null)
    setUploadOpen(false)
  }

  const handleApplyUrl = async () => {
    const url = urlInput.trim()
    if (!url) return
    await act(`url`, () => ipc.applySkinUrl(account.id, url, variant), '已应用 URL 皮肤')
    setUrlInput('')
  }

  const btnGhost =
    'rounded-md px-2.5 py-1 text-xs text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:text-neutral-400 dark:hover:bg-neutral-800'
  const btnAccent =
    'rounded-md bg-accent-soft px-2.5 py-1 text-xs font-medium text-accent transition-colors duration-150 hover:bg-accent hover:text-white disabled:opacity-50'

  return (
    <div className="animate-[slide-in-right_180ms_ease-out] space-y-4">
      {/* 头部 */}
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={onBack}
          className="rounded-lg px-2.5 py-1.5 text-sm text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
        >
          ← 返回
        </button>
        <h1 className="text-lg font-semibold">{account.name}</h1>
        <span
          className={`rounded px-1.5 py-0.5 text-[10px] ${
            isMicrosoft || isAuthlib
              ? 'bg-accent-soft text-accent'
              : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
          }`}
        >
          {isMicrosoft
            ? '微软正版'
            : isAuthlib
              ? `皮肤站${account.serverName ? ` · ${account.serverName}` : ''}`
              : '离线模式'}
        </span>
      </div>

      {message && (
        <div className="rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300">
          {message}
        </div>
      )}
      {error && (
        <div className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
          {error}
        </div>
      )}

      <div className="flex flex-col gap-4 lg:flex-row">
        {/* 3D 预览 */}
        <div className="rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900 lg:w-80 lg:shrink-0">
          <div className="mb-2 flex items-center justify-between">
            <p className="text-sm font-medium">预览</p>
            {isMicrosoft && activeCape && (
              <label className="flex cursor-pointer items-center gap-1.5 text-xs text-neutral-500 dark:text-neutral-400">
                <input
                  type="checkbox"
                  checked={showCape}
                  onChange={(e) => setShowCape(e.target.checked)}
                  className="size-3.5 accent-[var(--accent)]"
                />
                显示披风
              </label>
            )}
          </div>
          {isMicrosoft && skinTex ? (
            <div className="flex justify-center rounded-lg bg-gradient-to-b from-neutral-100 to-neutral-200 dark:from-neutral-800 dark:to-neutral-900">
              <SkinCanvas
                skinUrl={skinTex}
                capeUrl={capeTex}
                showCape={showCape}
                model={activeSkin?.variant ?? 'classic'}
              />
            </div>
          ) : isAuthlib && authlibSkin ? (
            <div className="flex justify-center rounded-lg bg-gradient-to-b from-neutral-100 to-neutral-200 dark:from-neutral-800 dark:to-neutral-900">
              <SkinCanvas
                skinUrl={authlibSkin.dataUrl}
                capeUrl={null}
                showCape={false}
                model={authlibSkin.model === 'slim' ? 'slim' : 'classic'}
              />
            </div>
          ) : !isMicrosoft && offlineSkinTex ? (
            <div className="flex justify-center rounded-lg bg-gradient-to-b from-neutral-100 to-neutral-200 dark:from-neutral-800 dark:to-neutral-900">
              <SkinCanvas
                skinUrl={offlineSkinTex}
                capeUrl={null}
                showCape={false}
                model={shownVariant}
              />
            </div>
          ) : (
            <div className="flex h-56 items-center justify-center rounded-lg bg-neutral-50 text-xs text-neutral-400 dark:bg-neutral-800/60 dark:text-neutral-500">
              {isMicrosoft
                ? '暂无皮肤贴图'
                : isAuthlib
                  ? '皮肤站未设置皮肤或加载中'
                  : offlineSkin
                    ? '皮肤贴图加载中…'
                    : '尚未设置离线皮肤'}
            </div>
          )}
          <p className="mt-2 text-center text-[10px] text-neutral-400 dark:text-neutral-500">
            拖拽旋转 · 滚轮缩放
          </p>
        </div>

        {/* 管理面板 */}
        <div className="min-w-0 flex-1 space-y-4">
          {isMicrosoft ? (
            <>
              {/* 皮肤 */}
              <div className="rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
                <div className="mb-3 flex items-center justify-between">
                  <p className="text-sm font-medium">皮肤</p>
                  <button
                    type="button"
                    onClick={openUploadModal}
                    className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
                  >
                    ＋ 添加皮肤
                  </button>
                </div>

                {/* 皮肤库 */}
                {account.skins.length === 0 ? (
                  <p className="py-3 text-center text-xs text-neutral-400 dark:text-neutral-500">
                    暂无皮肤记录，上传或稍后刷新
                  </p>
                ) : (
                  <div className="max-h-52 space-y-1 overflow-y-auto">
                    {account.skins.map((s) => {
                      const isActiveSkin = s.state === 'ACTIVE'
                      const key = `skin-${s.id}`
                      return (
                        <div
                          key={s.id}
                          className={`flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm ${
                            isActiveSkin
                              ? 'bg-accent-soft/40'
                              : 'hover:bg-neutral-50 dark:hover:bg-neutral-800/60'
                          }`}
                        >
                          <span className="min-w-0 flex-1 truncate">
                            {s.alias ?? `皮肤 ${s.id.slice(0, 8)}`}
                            <span className="ml-2 rounded bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400">
                              {s.variant === 'slim' ? '纤细' : '粗壮'}
                            </span>
                            {isActiveSkin && (
                              <span className="ml-2 text-[10px] text-accent">当前皮肤</span>
                            )}
                          </span>
                          {isActiveSkin ? (
                            <button
                              type="button"
                              disabled={busyKey !== null}
                              onClick={() =>
                                void act(
                                  key,
                                  () =>
                                    ipc.switchSkinVariant(
                                      account.id,
                                      s.variant === 'slim' ? 'classic' : 'slim',
                                    ),
                                  s.variant === 'slim' ? '已切换为粗壮模型' : '已切换为纤细模型',
                                )
                              }
                              className={btnGhost}
                            >
                              {busyKey === key ? '切换中…' : s.variant === 'slim' ? '改用粗壮' : '改用纤细'}
                            </button>
                          ) : (
                            <button
                              type="button"
                              disabled={busyKey !== null}
                              onClick={() =>
                                void act(key, () => ipc.applyLibrarySkin(account.id, s.id), '已应用该皮肤')
                              }
                              className={btnAccent}
                            >
                              {busyKey === key ? '应用中…' : '应用'}
                            </button>
                          )}
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>

              {/* 披风 */}
              <div className="rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
                <p className="mb-3 text-sm font-medium">披风（{account.capes.length}）</p>
                {account.capes.length === 0 ? (
                  <p className="py-3 text-center text-xs text-neutral-400 dark:text-neutral-500">
                    该账户暂未拥有任何披风
                  </p>
                ) : (
                  <div className="grid grid-cols-2 gap-2 md:grid-cols-3 xl:grid-cols-4">
                    {account.capes.map((c) => {
                      const isEquipped = c.id === account.activeCapeId && c.state === 'ACTIVE'
                      const key = `cape-${c.id}`
                      return (
                        <div
                          key={c.id}
                          className={`flex flex-col items-center gap-2 rounded-xl border px-3 py-3 transition-colors duration-150 ${
                            isEquipped
                              ? 'border-accent/60 bg-accent-soft/40'
                              : 'border-neutral-200 hover:border-accent/40 dark:border-neutral-800 dark:hover:border-accent/40'
                          }`}
                        >
                          <CapePreview url={c.url} />
                          <p className="w-full truncate text-center text-xs" title={c.alias ?? undefined}>
                            {c.alias ?? `披风 ${c.id.slice(0, 8)}`}
                          </p>
                          <button
                            type="button"
                            disabled={busyKey !== null}
                            onClick={() =>
                              void act(
                                key,
                                () =>
                                  isEquipped
                                    ? ipc.unequipCape(account.id)
                                    : ipc.equipCape(account.id, c.id),
                                isEquipped ? '已脱下披风' : '已装备披风',
                              )
                            }
                            className={isEquipped ? btnGhost : btnAccent}
                          >
                            {busyKey === key ? '处理中…' : isEquipped ? '脱下' : '装备'}
                          </button>
                        </div>
                      )
                    })}
                  </div>
                )}
              </div>
            </>
          ) : isAuthlib ? (
            /* 皮肤站账户：皮肤/披风由站点托管，仅展示信息与跳转 */
            <div className="rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
              <p className="text-sm font-medium">皮肤站账户</p>
              <div className="mt-2 space-y-1.5 text-xs text-neutral-500 dark:text-neutral-400">
                <p>
                  站点：
                  <span className="font-medium text-neutral-700 dark:text-neutral-200">
                    {account.serverName ?? '未知站点'}
                  </span>
                </p>
                {account.serverUrl && (
                  <p className="break-all font-mono text-[10px]">{account.serverUrl}</p>
                )}
                <p>
                  皮肤与披风由皮肤站托管，游戏内将自动使用站点上的外观；如需更换皮肤或披风，请前往站点官网操作。
                </p>
              </div>
              {account.serverUrl && (
                <button
                  type="button"
                  onClick={() => void ipc.openExternal(account.serverUrl!)}
                  className={`${btnAccent} mt-3`}
                >
                  前往站点管理
                </button>
              )}
            </div>
          ) : (
            /* 离线账户：预设 / 本地文件 / URL / 玩家名抓取，多来源皮肤注入 */
            <div className="rounded-xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
              <p className="text-sm font-medium">离线皮肤</p>
              {offlineSkin && (
                <div className="mt-2 flex items-center gap-2 rounded-lg bg-accent-soft/40 px-3 py-2 text-xs">
                  <span className="min-w-0 flex-1 truncate">
                    当前：{offlineSourceLabel(offlineSkin)}
                    <span className="ml-2 rounded bg-white px-1.5 py-0.5 text-[10px] text-neutral-600 dark:bg-neutral-700 dark:text-neutral-300">
                      {offlineSkin.variant === 'slim' ? '纤细' : '粗壮'}
                    </span>
                  </span>
                  <button
                    type="button"
                    disabled={busyKey !== null}
                    onClick={() => void handleRemoveOfflineSkin()}
                    className="shrink-0 rounded-md px-2 py-1 text-xs text-red-500 transition-colors duration-150 hover:bg-red-50 disabled:opacity-50 dark:hover:bg-red-500/10"
                  >
                    移除
                  </button>
                </div>
              )}

              {/* 预设 + 模型：暂存选择，点「应用设定」统一提交 */}
              <div className="mt-3 space-y-2">
                <div className="flex items-center gap-2">
                  <span className="w-10 shrink-0 text-xs text-neutral-500 dark:text-neutral-400">预设</span>
                  <div className="flex flex-1 gap-2">
                    {(['steve', 'alex'] as const).map((preset) => {
                      const active = shownPreset === preset
                      return (
                        <button
                          key={preset}
                          type="button"
                          disabled={busyKey !== null}
                          onClick={() => setPendingPreset(pendingPreset === preset ? null : preset)}
                          className={`flex-1 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors duration-150 disabled:opacity-50 ${
                            active
                              ? 'border-accent/60 bg-accent-soft text-accent'
                              : 'border-neutral-200 text-neutral-600 hover:border-accent/40 dark:border-neutral-700 dark:text-neutral-300'
                          }`}
                        >
                          {preset === 'steve' ? 'Steve' : 'Alex'}
                        </button>
                      )
                    })}
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-10 shrink-0 text-xs text-neutral-500 dark:text-neutral-400">模型</span>
                  <div className="flex flex-1 gap-2">
                    {(['classic', 'slim'] as const).map((v) => {
                      const active = shownVariant === v
                      return (
                        <button
                          key={v}
                          type="button"
                          disabled={busyKey !== null}
                          onClick={() => setPendingVariant(pendingVariant === v ? null : v)}
                          className={`flex-1 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors duration-150 disabled:opacity-50 ${
                            active
                              ? 'border-accent/60 bg-accent-soft text-accent'
                              : 'border-neutral-200 text-neutral-600 hover:border-accent/40 dark:border-neutral-700 dark:text-neutral-300'
                          }`}
                        >
                          {v === 'classic' ? '粗壮' : '纤细'}
                        </button>
                      )
                    })}
                  </div>
                  <button
                    type="button"
                    disabled={!settingsDirty || busyKey !== null}
                    onClick={() => void handleApplySettings()}
                    className={`shrink-0 rounded-lg px-3 py-1.5 text-xs font-medium transition-colors duration-150 disabled:opacity-50 ${
                      settingsDirty
                        ? 'bg-accent text-white hover:bg-accent-hover'
                        : 'bg-neutral-100 text-neutral-400 dark:bg-neutral-800'
                    }`}
                  >
                    {busyKey === 'apply-settings' ? '应用中…' : '应用设定'}
                  </button>
                </div>
              </div>

              {/* 本地文件 / URL */}
              <div className="mt-3 flex flex-wrap items-center gap-2">
                <button
                  type="button"
                  disabled={busyKey !== null}
                  onClick={() => void handleOfflineFile()}
                  className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
                >
                  {busyKey === 'offline-file' ? '处理中…' : '上传本地皮肤'}
                </button>
                <input
                  className="min-w-0 flex-1 rounded-lg border border-neutral-200 bg-transparent px-2.5 py-1.5 text-xs outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
                  placeholder="皮肤图片 URL（http/https，64x64 或 64x32 PNG）"
                  value={offlineUrlInput}
                  onChange={(e) => setOfflineUrlInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') void handleOfflineUrl()
                  }}
                />
                <button
                  type="button"
                  onClick={() => void handleOfflineUrl()}
                  disabled={busyKey !== null || !offlineUrlInput.trim()}
                  className="rounded-lg bg-neutral-100 px-3 py-1.5 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
                >
                  {busyKey === 'offline-url' ? '下载中…' : '从 URL 应用'}
                </button>
              </div>

              {/* 正版玩家名抓取（保留） */}
              <div className="mt-2 flex items-center gap-2">
                <input
                  className="min-w-0 flex-1 rounded-lg border border-neutral-200 bg-transparent px-2.5 py-1.5 text-xs outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
                  placeholder="或输入正版玩家名抓取其皮肤（如 Notch）"
                  value={playerNameInput}
                  onChange={(e) => setPlayerNameInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') void handleFetchOfflineSkin()
                  }}
                  maxLength={16}
                />
                <button
                  type="button"
                  onClick={() => void handleFetchOfflineSkin()}
                  disabled={busyKey !== null || !playerNameInput.trim()}
                  className="rounded-lg bg-neutral-100 px-3 py-1.5 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
                >
                  {busyKey === 'fetch-offline' ? '抓取中…' : '抓取并应用'}
                </button>
              </div>
              <p className="mt-2 text-[10px] text-neutral-400 dark:text-neutral-500">
                皮肤仅对你自己的客户端生效，启动该账户的游戏时自动注入（首次启动需联网下载注入组件）；不设置时游戏按 UUID 默认随机 Steve/Alex
              </p>
            </div>
          )}
        </div>
      </div>

      {/* 添加皮肤弹窗：拖拽上传 / 文件选择 / URL 应用（打开时接管全窗拖放） */}
      <Modal open={uploadOpen} onClose={closeUploadModal} title="添加皮肤">
        {/* 皮肤模型 */}
        <div className="mb-3 flex items-center gap-2">
          <span className="text-xs text-neutral-500 dark:text-neutral-400">皮肤模型</span>
          <div className="flex rounded-lg bg-neutral-100 p-0.5 dark:bg-neutral-800">
            {(
              [
                ['slim', '纤细'],
                ['classic', '粗壮'],
              ] as const
            ).map(([v, label]) => (
              <button
                key={v}
                type="button"
                onClick={() => setVariant(v)}
                className={`rounded-md px-3 py-1 text-xs transition-colors duration-150 ${
                  variant === v
                    ? 'bg-white font-medium text-neutral-800 shadow-sm dark:bg-neutral-700 dark:text-neutral-100'
                    : 'text-neutral-500 hover:text-neutral-700 dark:text-neutral-400 dark:hover:text-neutral-200'
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {/* 拖拽 / 点击选择区 */}
        <button
          type="button"
          onClick={async () => {
            const path = await ipc.pickSkinImage()
            if (path) void handleUpload(path)
          }}
          disabled={busyKey !== null}
          className="flex w-full flex-col items-center gap-2 rounded-xl border-2 border-dashed border-neutral-300 px-4 py-8 transition-colors duration-150 hover:border-accent hover:bg-accent-soft/20 disabled:opacity-50 dark:border-neutral-700 dark:hover:border-accent"
        >
          {busyKey?.startsWith('upload') ? (
            <span className="inline-block size-5 animate-spin rounded-full border-2 border-accent border-t-transparent" />
          ) : (
            <IconPlus className="size-6 text-neutral-400" />
          )}
          <span className="text-sm text-neutral-600 dark:text-neutral-300">
            {busyKey?.startsWith('upload') ? '上传中…' : '拖拽 PNG 皮肤到此处，或点击选择文件'}
          </span>
          <span className="text-xs text-neutral-400 dark:text-neutral-500">
            将以{variant === 'slim' ? '纤细' : '粗壮'}模型上传（64x64 或 64x32）
          </span>
        </button>

        {/* URL 应用 */}
        <div className="mt-4">
          <p className="mb-1.5 text-xs text-neutral-500 dark:text-neutral-400">或通过 URL 应用</p>
          <div className="flex items-center gap-2">
            <input
              className="min-w-0 flex-1 rounded-lg border border-neutral-200 bg-transparent px-2.5 py-1.5 text-xs outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
              placeholder="https://…（皮肤图片地址）"
              value={urlInput}
              onChange={(e) => setUrlInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void handleApplyUrl()
              }}
            />
            <button
              type="button"
              onClick={() => void handleApplyUrl()}
              disabled={busyKey !== null || !urlInput.trim()}
              className={btnAccent}
            >
              {busyKey === 'url' ? '应用中…' : '应用'}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  )
}
