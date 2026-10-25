// 玩家视图：账户管理（阶段六）
//
// 全局单一激活账户：启动任意实例均使用激活账户的认证参数。
// 账户列表点击「管理」进入独立账户详情页（3D 皮肤预览 / 皮肤库 / 上传 / 披风），
// 微软正版经设备码登录；离线皮肤（抓取正版玩家注入）在批次 D 提供。

import { useCallback, useEffect, useState } from 'react'
import { ipc, onAccountsChanged, onMsLoginState } from '../../lib/ipc'
import { useUiStore } from '../../stores/uiStore'
import type { AccountView } from '../../types/domain'
import { IconPlayer, IconPlus } from '../../components/common/Icons'
import { Modal } from '../../components/common/Modal'
import { AccountDetailView } from './AccountDetailView'

const KIND_LABEL: Record<string, string> = {
  offline: '离线模式',
  microsoft: '微软正版',
}

function formatTime(epochSec: number): string {
  if (epochSec <= 0) return '未知'
  return new Date(epochSec * 1000).toLocaleDateString()
}

/** 登录弹窗状态机：idle=请求中 code=展示设备码 pending=认证中 success/failed=终态 */
type LoginStage = 'idle' | 'code' | 'pending' | 'success' | 'failed'

export function PlayerView() {
  const [accounts, setAccounts] = useState<AccountView[] | null>(null)
  const [activeId, setActiveId] = useState<string | null>(null)
  const [newName, setNewName] = useState('')
  const [adding, setAdding] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  // 微软设备码登录弹窗
  const [loginOpen, setLoginOpen] = useState(false)
  const [loginStage, setLoginStage] = useState<LoginStage>('idle')
  const [loginCode, setLoginCode] = useState('')
  const [loginMsg, setLoginMsg] = useState('')
  const [copied, setCopied] = useState(false)
  /** 正在管理的账户 id（非 null = 显示独立账户管理页）；首页三个点经 uiStore 接力直达详情 */
  const [detailId, setDetailId] = useState<string | null>(
    () => useUiStore.getState().pendingAccountDetail,
  )
  // 消费接力值后立即清空（仅本次挂载生效）
  useEffect(() => {
    if (useUiStore.getState().pendingAccountDetail !== null) {
      useUiStore.getState().setPendingAccountDetail(null)
    }
  }, [])

  const refresh = useCallback(async () => {
    try {
      const r = await ipc.listAccounts()
      setAccounts(r.accounts)
      setActiveId(r.activeId)
    } catch (e) {
      setError(String(e))
    }
  }, [])

  // 初始加载 + 账户变更事件驱动刷新
  useEffect(() => {
    void refresh()
    let unlisten: (() => void) | undefined
    onAccountsChanged(() => refresh()).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [refresh])

  // 微软登录流程状态
  useEffect(() => {
    let unlisten: (() => void) | undefined
    onMsLoginState((p) => {
      switch (p.stage) {
        case 'code':
          setLoginStage('code')
          setLoginCode(p.userCode ?? '')
          setCopied(p.copied ?? false)
          break
        case 'pending':
          setLoginStage('pending')
          setLoginMsg(p.message ?? '正在完成 Minecraft 认证…')
          break
        case 'success':
          setLoginStage('success')
          setLoginMsg(p.message ?? '登录成功')
          void refresh()
          window.setTimeout(() => setLoginOpen(false), 1500)
          break
        case 'expired':
          setLoginStage('failed')
          setLoginMsg('设备码已过期，请重新发起登录')
          break
        case 'denied':
          setLoginStage('failed')
          setLoginMsg('您在浏览器中拒绝了授权')
          break
        case 'failed':
          setLoginStage('failed')
          setLoginMsg(p.message ?? '登录失败')
          break
      }
    }).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [refresh])

  const handleAdd = async () => {
    const name = newName.trim()
    if (!name) return
    setAdding(true)
    setError(null)
    setMessage(null)
    try {
      const acc = await ipc.addOfflineAccount(name)
      setNewName('')
      setMessage(`已添加离线账户「${acc.name}」并设为当前账户`)
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setAdding(false)
    }
  }

  const handleSwitch = async (id: string) => {
    if (id === activeId) return
    setError(null)
    setMessage(null)
    try {
      await ipc.setActiveAccount(id)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  const handleRemove = async (acc: AccountView) => {
    if (
      !window.confirm(
        `移除账户「${acc.name}」？\n${acc.kind === 'microsoft' ? '该微软账户的本地登录状态将被清除。' : '离线账户可随时重新添加。'}`,
      )
    )
      return
    setError(null)
    setMessage(null)
    try {
      await ipc.removeAccount(acc.id)
      setMessage(`已移除账户「${acc.name}」`)
      await refresh()
    } catch (e) {
      setError(String(e))
    }
  }

  /** 发起微软设备码登录：Rust 打开浏览器，弹窗展示设备码 */
  const startLogin = async () => {
    setLoginOpen(true)
    setLoginStage('idle')
    setLoginMsg('正在请求设备码…')
    try {
      await ipc.loginMicrosoft()
    } catch (e) {
      setLoginStage('failed')
      setLoginMsg(String(e))
    }
  }

  /** 关闭弹窗时同时取消后台轮询 */
  const closeLogin = () => {
    setLoginOpen(false)
    if (loginStage !== 'success') {
      void ipc.cancelMicrosoftLogin().catch(() => {})
    }
  }

  const copyCode = async () => {
    try {
      await navigator.clipboard.writeText(loginCode)
      setCopied(true)
    } catch {
      /* 剪贴板不可用时用户可手动选中复制 */
    }
  }

  const active = accounts?.find((a) => a.id === activeId) ?? null
  const inputCls =
    'rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'

  // 详情页目标账户（accounts-changed 刷新后自动拿到最新数据）
  const detailAccount = detailId ? (accounts?.find((a) => a.id === detailId) ?? null) : null

  // 独立账户管理页（3D 预览 / 皮肤 / 披风）
  if (detailAccount) {
    return (
      <div className="mx-auto max-w-4xl space-y-4 p-6">
        <AccountDetailView account={detailAccount} onBack={() => setDetailId(null)} />
      </div>
    )
  }

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-6">
      <h1 className="text-lg font-semibold">账户管理</h1>

      {/* 当前账户卡 */}
      <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
        {active ? (
          <div className="flex items-center gap-3">
            <span className="flex size-11 items-center justify-center rounded-lg bg-accent-soft text-accent">
              <IconPlayer className="size-6" />
            </span>
            <div className="min-w-0 flex-1">
              <p className="truncate font-medium">{active.name}</p>
              <p className="text-xs text-neutral-400 dark:text-neutral-500">
                {KIND_LABEL[active.kind] ?? active.kind} · 当前账户 · 启动实例时使用
              </p>
            </div>
          </div>
        ) : (
          <div className="flex items-center gap-3 text-neutral-500 dark:text-neutral-400">
            <span className="flex size-11 items-center justify-center rounded-lg bg-neutral-100 dark:bg-neutral-800">
              <IconPlayer className="size-6" />
            </span>
            <div>
              <p className="font-medium">尚未选择账户</p>
              <p className="text-xs text-neutral-400 dark:text-neutral-500">
                启动实例前需要先在下方添加并选择一个账户
              </p>
            </div>
          </div>
        )}
      </section>

      {/* 提示区 */}
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

      {/* 账户列表 */}
      <section className="space-y-2">
        <h2 className="text-sm font-medium text-neutral-600 dark:text-neutral-300">账户列表</h2>
        {accounts === null ? (
          <p className="py-6 text-center text-sm text-neutral-400">加载中…</p>
        ) : accounts.length === 0 ? (
          <div className="flex flex-col items-center gap-2 rounded-xl border border-dashed border-neutral-200 py-10 text-neutral-400 dark:border-neutral-800 dark:text-neutral-500">
            <IconPlayer className="size-10 opacity-40" />
            <p className="text-sm">还没有账户</p>
            <p className="text-xs">添加离线账户即可开始游玩，或在上方登录微软正版账户</p>
          </div>
        ) : (
          accounts.map((a) => {
            const isActive = a.id === activeId
            return (
              <div
                key={a.id}
                className={`flex items-center gap-3 rounded-xl border px-4 py-3 transition-colors duration-150 ${
                  isActive
                    ? 'border-accent/60 bg-accent-soft/40'
                    : 'border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900'
                }`}
              >
                <input
                  type="radio"
                  checked={isActive}
                  onChange={() => handleSwitch(a.id)}
                  aria-label={`切换到 ${a.name}`}
                  className="size-4 shrink-0 accent-[var(--accent)]"
                />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium">{a.name}</span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] ${
                        a.kind === 'microsoft'
                          ? 'bg-accent-soft text-accent'
                          : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
                      }`}
                    >
                      {KIND_LABEL[a.kind] ?? a.kind}
                    </span>
                    {isActive && (
                      <span className="text-[10px] text-accent">当前账户</span>
                    )}
                  </div>
                  <p className="mt-0.5 truncate font-mono text-xs text-neutral-400 dark:text-neutral-500">
                    {a.uuid} · 添加于 {formatTime(a.createdAt)}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => setDetailId(a.id)}
                  className="shrink-0 rounded-lg px-2.5 py-1.5 text-xs text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
                >
                  管理
                </button>
                <button
                  type="button"
                  onClick={() => handleRemove(a)}
                  className="shrink-0 rounded-lg px-2.5 py-1.5 text-xs text-neutral-400 transition-colors duration-150 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10"
                >
                  移除
                </button>
              </div>
            )
          })
        )}
      </section>

      {/* 登录方式 */}
      <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
        <h2 className="text-sm font-medium">微软正版登录</h2>
        <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
          通过设备码在系统浏览器中登录微软账户（启动器不会接触你的密码），登录后可使用正版启动与皮肤管理
        </p>
        <button
          type="button"
          onClick={startLogin}
          className="mt-3 flex items-center gap-1.5 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
        >
          <IconPlus className="size-4" />
          登录微软账户
        </button>
      </section>

      {/* 添加离线账户 */}
      <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
        <h2 className="text-sm font-medium">添加离线账户</h2>
        <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
          离线模式使用自定义 ID，仅适用于离线启动与多数第三方服务器
        </p>
        <div className="mt-3 flex items-center gap-3">
          <input
            className={`${inputCls} min-w-0 flex-1`}
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void handleAdd()
            }}
            placeholder="玩家 ID（3–16 位字母、数字或下划线）"
            aria-label="离线玩家 ID"
            maxLength={16}
          />
          <button
            type="button"
            onClick={handleAdd}
            disabled={adding || !newName.trim()}
            className="flex shrink-0 items-center gap-1.5 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            <IconPlus className="size-4" />
            {adding ? '添加中…' : '添加'}
          </button>
        </div>
      </section>

      {/* 微软设备码登录弹窗（dismissible=false：点遮罩/Esc 不关闭，防误触取消流程） */}
      <Modal open={loginOpen} onClose={closeLogin} title="登录微软账户" dismissible={false}>
        {loginStage === 'code' ? (
          <div className="space-y-3 text-center">
            <p className="text-sm text-neutral-600 dark:text-neutral-300">
              浏览器已打开微软授权页，请登录并输入以下代码：
            </p>
            <button
              type="button"
              onClick={copyCode}
              title="点击复制"
              className="mx-auto block w-full cursor-pointer rounded-xl border border-dashed border-accent/50 bg-accent-soft/30 px-4 py-4 font-mono text-2xl font-semibold tracking-[0.3em] text-accent transition-colors duration-150 hover:bg-accent-soft/60"
            >
              {loginCode}
            </button>
            <p className="text-xs text-neutral-400 dark:text-neutral-500">
              {copied ? '已复制到剪贴板' : '点击代码即可复制'}
            </p>
            <div className="flex items-center justify-center gap-2 text-xs text-neutral-500 dark:text-neutral-400">
              <span className="inline-block size-3 animate-pulse rounded-full bg-accent" />
              正在等待浏览器中完成授权…
            </div>
          </div>
        ) : loginStage === 'pending' || loginStage === 'idle' ? (
          <div className="flex flex-col items-center gap-3 py-6 text-sm text-neutral-500 dark:text-neutral-400">
            <span className="inline-block size-6 animate-spin rounded-full border-2 border-accent border-t-transparent" />
            {loginMsg || '正在处理…'}
          </div>
        ) : loginStage === 'success' ? (
          <div className="flex flex-col items-center gap-2 py-6 text-sm text-emerald-600 dark:text-emerald-400">
            <span className="text-2xl">✓</span>
            {loginMsg}
          </div>
        ) : (
          <div className="space-y-3">
            <p className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
              {loginMsg || '登录失败'}
            </p>
            <button
              type="button"
              onClick={startLogin}
              className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
            >
              重新发起登录
            </button>
          </div>
        )}
      </Modal>
    </div>
  )
}
