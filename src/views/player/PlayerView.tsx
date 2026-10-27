// 玩家视图：账户管理（阶段六）
//
// 全局单一激活账户：启动任意实例均使用激活账户的认证参数。
// 账户列表点击「管理」进入独立账户详情页（3D 皮肤预览 / 皮肤库 / 上传 / 披风），
// 微软正版经设备码登录；离线皮肤（抓取正版玩家注入）在批次 D 提供。

import { useCallback, useEffect, useState } from 'react'
import { ipc, onAccountsChanged, onAuthlibOauthState, onMsLoginState } from '../../lib/ipc'
import { useUiStore } from '../../stores/uiStore'
import type { AccountView, AuthlibProfile, AuthlibServerMeta } from '../../types/domain'
import { IconPlayer, IconPlus } from '../../components/common/Icons'
import { Modal } from '../../components/common/Modal'
import { AccountAvatar } from './AccountAvatar'
import { AccountDetailView } from './AccountDetailView'

const KIND_LABEL: Record<string, string> = {
  offline: '离线模式',
  microsoft: '微软正版',
  authlib: '皮肤站',
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
      setOfflineOpen(false)
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
    const hint =
      acc.kind === 'microsoft'
        ? '该微软账户的本地登录状态将被清除。'
        : acc.kind === 'authlib'
          ? '将尝试吊销皮肤站登录令牌。'
          : '离线账户可随时重新添加。'
    if (!window.confirm(`移除账户「${acc.name}」？\n${hint}`)) return
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

  // 皮肤站登录流程（分步：服务器地址 → 账密登录 → 多角色选择）
  type AuthlibStep = 'server' | 'login' | 'profiles' | 'done' | 'failed'
  const [authlibOpen, setAuthlibOpen] = useState(false)
  const [authlibStep, setAuthlibStep] = useState<AuthlibStep>('server')
  const [authlibUrl, setAuthlibUrl] = useState('https://littleskin.cn')
  const [authlibMeta, setAuthlibMeta] = useState<AuthlibServerMeta | null>(null)
  const [authlibUsername, setAuthlibUsername] = useState('')
  const [authlibPassword, setAuthlibPassword] = useState('')
  const [authlibProfiles, setAuthlibProfiles] = useState<AuthlibProfile[]>([])
  const [authlibBusy, setAuthlibBusy] = useState(false)
  const [authlibMsg, setAuthlibMsg] = useState('')

  const openAuthlib = () => {
    setAuthlibOpen(true)
    setAuthlibStep('server')
    setAuthlibMeta(null)
    setAuthlibUrl('https://littleskin.cn')
    setAuthlibUsername('')
    setAuthlibPassword('')
    setAuthlibProfiles([])
    setAuthlibBusy(false)
    setAuthlibMsg('')
  }

  /** 第一步：解析站点元数据（确认站点存在 + 拿站名/注册链接） */
  const checkAuthlibServer = async () => {
    setAuthlibBusy(true)
    setAuthlibMsg('')
    try {
      const meta = await ipc.fetchAuthlibServerMeta(authlibUrl)
      setAuthlibMeta(meta)
      setAuthlibStep('login')
    } catch (e) {
      setAuthlibMsg(String(e))
    } finally {
      setAuthlibBusy(false)
    }
  }

  /** 第二步：账密登录；单角色直接创建账户，多角色进入选择 */
  const submitAuthlibLogin = async () => {
    if (!authlibMeta) return
    setAuthlibBusy(true)
    setAuthlibMsg('')
    try {
      const r = await ipc.loginAuthlib(authlibUrl, authlibUsername, authlibPassword)
      if (r.account) {
        setAuthlibStep('done')
        setMessage(`已添加皮肤站角色「${r.account.name}」并设为当前账户`)
        await refresh()
        window.setTimeout(() => setAuthlibOpen(false), 1200)
      } else {
        setAuthlibProfiles(r.profiles)
        setAuthlibStep('profiles')
      }
    } catch (e) {
      setAuthlibMsg(String(e))
    } finally {
      setAuthlibBusy(false)
    }
  }

  /** 第三步：绑定所选角色 */
  const pickAuthlibProfile = async (p: AuthlibProfile) => {
    setAuthlibBusy(true)
    setAuthlibMsg('')
    try {
      const acc = await ipc.confirmAuthlibProfile(p.id)
      setAuthlibStep('done')
      setMessage(`已添加皮肤站角色「${acc.name}」并设为当前账户`)
      await refresh()
      window.setTimeout(() => setAuthlibOpen(false), 1200)
    } catch (e) {
      setAuthlibMsg(String(e))
    } finally {
      setAuthlibBusy(false)
    }
  }

  // LittleSkin OAuth 免密授权（设备码流，仅 LittleSkin 域名显示入口）
  const isLittleSkin = authlibUrl.toLowerCase().includes('littleskin.cn')

  // 「添加账户」下拉菜单与离线账户弹窗
  const [addMenuOpen, setAddMenuOpen] = useState(false)
  const [offlineOpen, setOfflineOpen] = useState(false)
  const openOfflineAdd = () => {
    setOfflineOpen(true)
    setNewName('')
    setError(null)
  }
  type OauthStage = 'idle' | 'code' | 'pending' | 'success' | 'failed'
  const [oauthOpen, setOauthOpen] = useState(false)
  const [oauthStage, setOauthStage] = useState<OauthStage>('idle')
  const [oauthCode, setOauthCode] = useState('')
  const [oauthMsg, setOauthMsg] = useState('')
  const [oauthCopied, setOauthCopied] = useState(false)

  useEffect(() => {
    let unlisten: (() => void) | undefined
    onAuthlibOauthState((p) => {
      switch (p.stage) {
        case 'code':
          setOauthStage('code')
          setOauthCode(p.userCode ?? '')
          setOauthCopied(p.copied ?? false)
          break
        case 'pending':
          setOauthStage('pending')
          setOauthMsg(p.message ?? '正在等待授权…')
          break
        case 'success':
          setOauthStage('success')
          setOauthMsg(p.message ?? '登录成功')
          void refresh()
          window.setTimeout(() => setOauthOpen(false), 1500)
          break
        case 'failed':
          setOauthStage('failed')
          setOauthMsg(p.message ?? '登录失败')
          break
      }
    }).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [refresh])

  /** 发起免密授权：Rust 打开授权页，弹窗展示授权码 */
  const startOauth = async () => {
    setOauthOpen(true)
    setOauthStage('idle')
    setOauthMsg('正在请求授权码…')
    try {
      await ipc.loginLittleskinOauth(authlibUrl)
    } catch (e) {
      setOauthStage('failed')
      setOauthMsg(String(e))
    }
  }

  /** 关闭弹窗时同时取消后台轮询 */
  const closeOauth = () => {
    setOauthOpen(false)
    if (oauthStage !== 'success') {
      void ipc.cancelLittleskinOauth().catch(() => {})
    }
  }

  const copyOauthCode = async () => {
    try {
      await navigator.clipboard.writeText(oauthCode)
      setOauthCopied(true)
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
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold">账户管理</h1>
        {/* 添加账户：按钮 + 方式选择菜单 */}
        <div className="relative">
          <button
            type="button"
            onClick={() => setAddMenuOpen((v) => !v)}
            className="flex items-center gap-1.5 rounded-lg bg-accent px-3.5 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
          >
            <IconPlus className="size-4" />
            添加账户
          </button>
          {addMenuOpen && (
            <>
              {/* 点击菜单外区域关闭 */}
              <div className="fixed inset-0 z-30" onClick={() => setAddMenuOpen(false)} />
              <div className="absolute right-0 top-full z-40 mt-2 w-80 overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-lg dark:border-neutral-700 dark:bg-neutral-800">
                {(
                  [
                    ['皮肤站登录', '支持 LittleSkin 等 authlib 皮肤站，可免密授权', openAuthlib],
                    ['微软正版登录', '设备码免密授权，启动器不接触密码', startLogin],
                    ['添加离线账户', '自定义玩家 ID，适用于离线启动与第三方服务器', () => openOfflineAdd()],
                  ] as const
                ).map(([title, desc, action]) => (
                  <button
                    key={title}
                    type="button"
                    onClick={() => {
                      setAddMenuOpen(false)
                      action()
                    }}
                    className="block w-full px-4 py-3 text-left transition-colors duration-150 hover:bg-neutral-100 dark:hover:bg-neutral-700/60"
                  >
                    <p className="text-sm font-medium">{title}</p>
                    <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">{desc}</p>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      </div>

      {/* 当前账户 Hero 卡 */}
      <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
        {active ? (
          <div className="flex items-center gap-3">
            <AccountAvatar account={active} size={44} />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <p className="truncate font-medium">{active.name}</p>
                <span className="rounded bg-accent-soft px-1.5 py-0.5 text-[10px] text-accent">
                  {KIND_LABEL[active.kind] ?? active.kind}
                </span>
                {active.kind === 'authlib' && active.serverName && (
                  <span className="truncate text-[10px] text-neutral-400 dark:text-neutral-500">
                    {active.serverName}
                  </span>
                )}
              </div>
              <p className="text-xs text-neutral-400 dark:text-neutral-500">当前账户 · 启动实例时使用</p>
            </div>
            <div className="flex shrink-0 gap-2">
              <button
                type="button"
                onClick={() => setDetailId(active.id)}
                className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
              >
                管理
              </button>
              <button
                type="button"
                onClick={() => handleRemove(active)}
                className="rounded-lg px-2.5 py-1.5 text-xs text-neutral-400 transition-colors duration-150 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10"
              >
                移除
              </button>
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
            <p className="text-xs">点击下方「添加账户」开始</p>
          </div>
        ) : (
          accounts.map((a) => {
            const isActive = a.id === activeId
            return (
              <div
                key={a.id}
                className={`flex items-center gap-3 rounded-xl border px-4 py-2.5 transition-colors duration-150 ${
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
                <AccountAvatar account={a} size={32} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate text-sm font-medium">{a.name}</span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] ${
                        a.kind === 'microsoft' || a.kind === 'authlib'
                          ? 'bg-accent-soft text-accent'
                          : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
                      }`}
                    >
                      {a.kind === 'authlib' && a.serverName
                        ? `皮肤站 · ${a.serverName}`
                        : (KIND_LABEL[a.kind] ?? a.kind)}
                    </span>
                  </div>
                  <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
                    添加于 {formatTime(a.createdAt)}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={() => setDetailId(a.id)}
                  className="shrink-0 rounded-lg px-2.5 py-1.5 text-xs text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800"
                >
                  管理
                </button>
                {!isActive && (
                  <button
                    type="button"
                    onClick={() => handleRemove(a)}
                    className="shrink-0 rounded-lg px-2.5 py-1.5 text-xs text-neutral-400 transition-colors duration-150 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10"
                  >
                    移除
                  </button>
                )}
              </div>
            )
          })
        )}
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

      {/* 皮肤站登录弹窗（分步：server=站点地址 login=账密 profiles=选角色） */}
      <Modal
        open={authlibOpen}
        onClose={() => !authlibBusy && setAuthlibOpen(false)}
        title="登录皮肤站"
      >
        {authlibStep === 'server' ? (
          <div className="space-y-3">
            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              输入皮肤站服务器地址（需支持 authlib-injector 规范）
            </p>
            <input
              className={`${inputCls} w-full`}
              value={authlibUrl}
              onChange={(e) => setAuthlibUrl(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && !authlibBusy && void checkAuthlibServer()}
              placeholder="https://littleskin.cn"
              aria-label="皮肤站服务器地址"
            />
            {authlibMsg && (
              <p className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
                {authlibMsg}
              </p>
            )}
            <button
              type="button"
              onClick={checkAuthlibServer}
              disabled={authlibBusy || !authlibUrl.trim()}
              className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
            >
              {authlibBusy ? '正在连接…' : '下一步'}
            </button>
          </div>
        ) : authlibStep === 'login' && authlibMeta ? (
          <div className="space-y-3">
            <div className="flex items-center justify-between rounded-lg bg-accent-soft/40 px-3 py-2">
              <div className="min-w-0">
                <p className="truncate text-sm font-medium">{authlibMeta.serverName}</p>
                <p className="truncate font-mono text-[10px] text-neutral-400 dark:text-neutral-500">
                  {authlibUrl}
                </p>
              </div>
              <button
                type="button"
                onClick={() => {
                  setAuthlibStep('server')
                  setAuthlibMsg('')
                }}
                className="shrink-0 text-xs text-neutral-400 hover:text-accent"
              >
                更换站点
              </button>
            </div>
            {isLittleSkin && (
              <div className="space-y-1.5">
                <button
                  type="button"
                  onClick={startOauth}
                  className="w-full rounded-lg bg-accent px-4 py-2.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
                >
                  免密授权登录（推荐）
                </button>
                <p className="text-center text-[10px] text-neutral-400 dark:text-neutral-500">
                  跳转浏览器完成授权并选择角色，无需在启动器输入密码；也可使用下方账号密码登录
                </p>
              </div>
            )}
            {authlibMeta.registerUrl && (
              <p className="text-xs text-neutral-500 dark:text-neutral-400">
                还没有账号？
                <button
                  type="button"
                  onClick={() => void ipc.openExternal(authlibMeta.registerUrl!)}
                  className="text-accent hover:underline"
                >
                  前往站点注册
                </button>
              </p>
            )}
            <input
              className={`${inputCls} w-full`}
              value={authlibUsername}
              onChange={(e) => setAuthlibUsername(e.target.value)}
              placeholder="账号（邮箱或用户名）"
              aria-label="皮肤站账号"
              autoComplete="off"
            />
            <input
              className={`${inputCls} w-full`}
              type="password"
              value={authlibPassword}
              onChange={(e) => setAuthlibPassword(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && !authlibBusy && void submitAuthlibLogin()}
              placeholder="密码"
              aria-label="皮肤站密码"
            />
            {authlibMsg && (
              <p className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
                {authlibMsg}
              </p>
            )}
            <button
              type="button"
              onClick={submitAuthlibLogin}
              disabled={authlibBusy || !authlibUsername.trim() || !authlibPassword}
              className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
            >
              {authlibBusy ? '正在登录…' : '登录'}
            </button>
          </div>
        ) : authlibStep === 'profiles' ? (
          <div className="space-y-3">
            <p className="text-sm text-neutral-600 dark:text-neutral-300">
              该账号下有多个角色，请选择要添加的角色：
            </p>
            <div className="max-h-52 space-y-2 overflow-y-auto">
              {authlibProfiles.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => void pickAuthlibProfile(p)}
                  disabled={authlibBusy}
                  className="flex w-full items-center justify-between rounded-lg border border-neutral-200 px-3 py-2.5 text-sm transition-colors duration-150 hover:border-accent/60 hover:bg-accent-soft/40 disabled:opacity-50 dark:border-neutral-700"
                >
                  <span className="font-medium">{p.name}</span>
                  <span className="font-mono text-[10px] text-neutral-400">{p.id}</span>
                </button>
              ))}
            </div>
            {authlibMsg && (
              <p className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
                {authlibMsg}
              </p>
            )}
          </div>
        ) : authlibStep === 'done' ? (
          <div className="flex flex-col items-center gap-2 py-6 text-sm text-emerald-600 dark:text-emerald-400">
            <span className="text-2xl">✓</span>
            登录成功
          </div>
        ) : (
          <div className="space-y-3">
            <p className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
              {authlibMsg || '登录失败'}
            </p>
            <button
              type="button"
              onClick={openAuthlib}
              className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
            >
              重新开始
            </button>
          </div>
        )}
      </Modal>

      {/* 添加离线账户弹窗 */}
      <Modal open={offlineOpen} onClose={() => !adding && setOfflineOpen(false)} title="添加离线账户">
        <div className="space-y-3">
          <p className="text-xs text-neutral-500 dark:text-neutral-400">
            离线模式使用自定义 ID，仅适用于离线启动与多数第三方服务器
          </p>
          <input
            className={`${inputCls} w-full`}
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void handleAdd()
            }}
            placeholder="玩家 ID（3–16 位字母、数字或下划线）"
            aria-label="离线玩家 ID"
            maxLength={16}
            autoFocus
          />
          <button
            type="button"
            onClick={handleAdd}
            disabled={adding || !newName.trim()}
            className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            {adding ? '添加中…' : '添加'}
          </button>
        </div>
      </Modal>

      {/* LittleSkin 免密授权弹窗（dismissible=false：防误触取消轮询） */}
      <Modal open={oauthOpen} onClose={closeOauth} title="LittleSkin 免密授权" dismissible={false}>
        {oauthStage === 'code' ? (
          <div className="space-y-3 text-center">
            <p className="text-sm text-neutral-600 dark:text-neutral-300">
              浏览器已打开 LittleSkin 授权页，请登录并输入以下授权码：
            </p>
            <button
              type="button"
              onClick={copyOauthCode}
              title="点击复制"
              className="mx-auto block w-full cursor-pointer rounded-xl border border-dashed border-accent/50 bg-accent-soft/30 px-4 py-4 font-mono text-2xl font-semibold tracking-[0.2em] text-accent transition-colors duration-150 hover:bg-accent-soft/60"
            >
              {oauthCode}
            </button>
            <p className="text-xs text-neutral-400 dark:text-neutral-500">
              {oauthCopied ? '已复制到剪贴板' : '点击授权码即可复制'}
            </p>
            <div className="flex items-center justify-center gap-2 text-xs text-neutral-500 dark:text-neutral-400">
              <span className="inline-block size-3 animate-pulse rounded-full bg-accent" />
              正在等待浏览器中完成授权…
            </div>
          </div>
        ) : oauthStage === 'pending' || oauthStage === 'idle' ? (
          <div className="flex flex-col items-center gap-3 py-6 text-sm text-neutral-500 dark:text-neutral-400">
            <span className="inline-block size-6 animate-spin rounded-full border-2 border-accent border-t-transparent" />
            {oauthMsg || '正在处理…'}
          </div>
        ) : oauthStage === 'success' ? (
          <div className="flex flex-col items-center gap-2 py-6 text-sm text-emerald-600 dark:text-emerald-400">
            <span className="text-2xl">✓</span>
            {oauthMsg}
          </div>
        ) : (
          <div className="space-y-3">
            <p className="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-600 dark:bg-red-500/10 dark:text-red-300">
              {oauthMsg || '登录失败'}
            </p>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={startOauth}
                className="flex-1 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
              >
                重试
              </button>
              <button
                type="button"
                onClick={closeOauth}
                className="flex-1 rounded-lg bg-neutral-100 px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
              >
                改用账密登录
              </button>
            </div>
          </div>
        )}
      </Modal>
    </div>
  )
}
