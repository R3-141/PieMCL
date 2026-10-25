// 联机页：樱花内网穿透（natfrp.com）一站式管理
//
// 流程：frpc 就绪（自动下载 / 手动指定）→ 隧道列表（启动器内直接新建/选择）
// → 启动穿透 → 连接地址卡展示 IPv4 直连地址发给朋友。
// 隧道列表经樱花 API 实时拉取（10s 轮询在线状态）；frpc 日志经 natfrp-log 实时展示。

import { useCallback, useEffect, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { ipc } from '../../lib/ipc'
import type { NatFrpAddr, NatFrpNode, NatFrpStatus, NatFrpTunnel } from '../../types/domain'
import { Modal } from '../../components/common/Modal'
import {
  IconClose,
  IconFolder,
  IconLan,
  IconPlay,
} from '../../components/common/Icons'

const sectionCls = 'rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900'
const inputCls = 'w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'
const btnGhostCls =
  'rounded-lg border border-neutral-200 px-3 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800'

/** 首次打开联机页的免责声明确认标记（localStorage 持久化） */
const DISCLAIMER_KEY = 'piemcl-lan-disclaimer-ack'

/** 隧道类型徽章 */
function TypeBadge({ t }: { t: string }) {
  const cls =
    t === 'tcp'
      ? 'bg-sky-100 text-sky-700 dark:bg-sky-500/15 dark:text-sky-300'
      : t === 'udp'
        ? 'bg-violet-100 text-violet-700 dark:bg-violet-500/15 dark:text-violet-300'
        : t === 'http'
          ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300'
          : 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300'
  return (
    <span className={`shrink-0 rounded px-1.5 py-0.5 text-[11px] font-semibold uppercase ${cls}`}>
      {t}
    </span>
  )
}

/** 地址展示值：原样地址（域名:端口 / IP:端口，两种形式均展示且可复制） */
const shownOf = (a: NatFrpAddr) => a.address

/** 合并地址并按原始地址去重 */
function mergeAddr(list: NatFrpAddr[], next: NatFrpAddr): NatFrpAddr[] {
  if (list.some((x) => x.address === next.address)) return list
  return [...list, next]
}

/** 条目展开：域名地址解析出的 IPv4 追加为独立条目（无日志场景也有 IP 形式可复制） */
function expandAddr(a: NatFrpAddr): NatFrpAddr[] {
  const ip = a.ipv4[0]
  if (ip && ip !== a.address) return [a, { address: ip, ipv4: [ip] }]
  return [a]
}

export function LanView() {
  const [status, setStatus] = useState<NatFrpStatus | null>(null)
  const [accessKey, setAccessKey] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const [logs, setLogs] = useState<string[]>([])
  // 自动获取的连接地址（日志 >>host:port<< 定界 + 隧道 API 轮询，IPv4 已解析）
  const [addrs, setAddrs] = useState<NatFrpAddr[]>([])
  const [copied, setCopied] = useState<string | null>(null)
  const logRef = useRef<HTMLDivElement>(null)
  const running = status?.running ?? false
  const ready = status?.frpcReady ?? false

  // ---- 隧道列表 ----
  const [tunnels, setTunnels] = useState<NatFrpTunnel[] | null>(null)
  const [tunnelsErr, setTunnelsErr] = useState<string | null>(null)
  const [tunnelsLoading, setTunnelsLoading] = useState(false)
  // 当前运行的隧道（启动器会话内记忆；frpc 全局单实例）
  const [runningTunnelId, setRunningTunnelId] = useState<number | null>(null)
  // 两步删除确认（记录待删除的隧道 id，超时自动还原）
  const [pendingDeleteId, setPendingDeleteId] = useState<number | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  // 首次打开联机页的免责声明弹窗（必须阅读 5 秒才能关闭）
  const [showDisclaimer, setShowDisclaimer] = useState(
    () => localStorage.getItem(DISCLAIMER_KEY) !== '1',
  )
  const [disclaimerCountdown, setDisclaimerCountdown] = useState(5)

  // 免责声明倒计时
  useEffect(() => {
    if (!showDisclaimer || disclaimerCountdown <= 0) return
    const t = setTimeout(() => setDisclaimerCountdown((c) => c - 1), 1000)
    return () => clearTimeout(t)
  }, [showDisclaimer, disclaimerCountdown])

  const ackDisclaimer = () => {
    localStorage.setItem(DISCLAIMER_KEY, '1')
    setShowDisclaimer(false)
  }

  const refresh = useCallback(async () => {
    try {
      const s = await ipc.natfrpStatus()
      setStatus(s)
      return s
    } catch (e) {
      setError(String(e))
      return null
    }
  }, [])

  const loadTunnels = useCallback(async () => {
    setTunnelsLoading(true)
    try {
      const list = await ipc.natfrpListTunnels()
      setTunnels(list)
      setTunnelsErr(null)
    } catch (e) {
      setTunnelsErr(String(e).replace(/^Error:\s*/i, ''))
      setTunnels(null)
    } finally {
      setTunnelsLoading(false)
    }
  }, [])

  useEffect(() => {
    // 首次加载：状态 + 已保存的访问密钥回显（输入框默认为空，用户改过则不覆盖）
    void refresh().then((s) => {
      if (s) setAccessKey((cur) => (cur === '' ? s.accessKey : cur))
    })
    // 事件监听：StrictMode 下 effect 双跑，listen 的 promise 可能晚于 cleanup
    // resolve，只靠 unlisten 变量会泄漏首个监听器（日志每行收两次），用 disposed 兜底
    let disposed = false
    const unlistens: Array<() => void> = []
    const reg = <T,>(event: string, handler: (payload: T) => void) => {
      void listen<T>(event, (e) => handler(e.payload)).then((u) => {
        if (disposed) u()
        else unlistens.push(u)
      })
    }
    reg<{ line: string }>('natfrp-log', ({ line }) => {
      setLogs((l) => [...l.slice(-500), line])
    })
    reg<{ running: boolean; code: number | null }>('natfrp-state', (p) => {
      if (!p.running) {
        setLogs((l) => [...l.slice(-500), `—— 穿透已停止（退出码 ${p.code ?? '未知'}）——`])
        setRunningTunnelId(null)
      }
      void refresh()
    })
    reg<NatFrpAddr>('natfrp-addr', (p) => {
      setAddrs((a) => expandAddr(p).reduce(mergeAddr, a))
    })
    return () => {
      disposed = true
      unlistens.forEach((u) => u())
    }
  }, [refresh])

  // 密钥已配置时轮询隧道列表（10s），启动/删除/创建后立即刷新由调用方触发
  useEffect(() => {
    if (!status?.accessKeySet) return
    void loadTunnels()
    const timer = setInterval(() => void loadTunnels(), 10000)
    return () => clearInterval(timer)
  }, [status?.accessKeySet, loadTunnels])

  // 两步删除确认超时还原
  useEffect(() => {
    if (pendingDeleteId === null) return
    const t = setTimeout(() => setPendingDeleteId(null), 3000)
    return () => clearTimeout(t)
  }, [pendingDeleteId])

  const copyAddr = useCallback(async (text: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(text)
      setTimeout(() => setCopied(null), 1500)
    } catch {
      /* 剪贴板不可用：忽略 */
    }
  }, [])

  // 直达樱花面板页面（系统默认浏览器）
  const openPage = useCallback((url: string) => {
    void ipc.natfrpOpenPage(url).catch((e) => setError(String(e)))
  }, [])

  const handleSave = async () => {
    setBusy(true)
    setError(null)
    try {
      await ipc.natfrpSaveConfig(accessKey, '', status?.frpcCustom ? status.frpcPath : null)
      await refresh()
      setSaved(true)
      void loadTunnels()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleDownload = async () => {
    setBusy(true)
    setError(null)
    try {
      await ipc.natfrpDownloadFrpc()
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const handlePick = async () => {
    setBusy(true)
    setError(null)
    try {
      await ipc.natfrpPickFrpc()
      await refresh()
    } catch {
      /* 取消选择：静默 */
    } finally {
      setBusy(false)
    }
  }

  const handleStartTunnel = async (t: NatFrpTunnel) => {
    setBusy(true)
    setError(null)
    try {
      // 启动前顺手持久化密钥（隧道 ID 无需手填）
      await ipc.natfrpSaveConfig(accessKey, '', status?.frpcCustom ? status.frpcPath : null)
      setAddrs([]) // 新一轮穿透：清空旧地址
      await ipc.natfrpStart(String(t.id))
      setRunningTunnelId(t.id)
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleStop = async () => {
    setBusy(true)
    setError(null)
    try {
      await ipc.natfrpStop()
      await refresh()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const handleDeleteTunnel = async (t: NatFrpTunnel) => {
    if (pendingDeleteId !== t.id) {
      setPendingDeleteId(t.id)
      return
    }
    setPendingDeleteId(null)
    setBusy(true)
    setError(null)
    try {
      await ipc.natfrpDeleteTunnel(t.id)
      await loadTunnels()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  const keySet = status?.accessKeySet ?? false

  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold">联机</h1>
          <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
            通过樱花内网穿透（natfrp.com）把本机游戏开放给好友联机，需要 natfrp.com 账号
          </p>
        </div>
      </div>

      {error && <p className="text-sm text-red-500">{error}</p>}

      {/* 穿透状态卡 */}
      <section className={sectionCls}>
        <div className="flex flex-wrap items-center gap-3">
          <span
            className={`flex items-center gap-1.5 rounded-md px-2 py-1 text-xs font-medium ${
              running
                ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-300'
                : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
            }`}
          >
            <span className={`size-1.5 rounded-full ${running ? 'animate-pulse bg-emerald-500' : 'bg-neutral-400'}`} />
            {running ? '穿透运行中' : '穿透未运行'}
          </span>
          <span className="text-xs text-neutral-500 dark:text-neutral-400">
            frpc：
            {ready ? (
              <span className="text-emerald-600 dark:text-emerald-400">已就绪</span>
            ) : (
              <span className="text-red-500">未就绪</span>
            )}
            {status?.frpcCustom && '（手动指定）'}
            {keySet ? ' · 密钥已配置' : ' · 未配置密钥'}
          </span>
          {running && (
            <button
              type="button"
              onClick={() => void handleStop()}
              disabled={busy}
              className="ml-auto rounded-lg bg-red-500 px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-red-600 disabled:opacity-50"
            >
              停止穿透
            </button>
          )}
        </div>
      </section>

      {/* 隧道列表卡 */}
      <section className={sectionCls}>
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <h2 className="flex items-center gap-1.5 text-sm font-semibold">
            <IconLan className="size-4" />
            隧道列表
          </h2>
          <div className="flex items-center gap-3">
            {tunnelsLoading && <span className="text-xs text-neutral-400">刷新中…</span>}
            <button
              type="button"
              onClick={() => setShowCreate(true)}
              disabled={!keySet || busy}
              title={!keySet ? '请先保存访问密钥' : undefined}
              className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
            >
              新建隧道
            </button>
          </div>
        </div>

        {!keySet ? (
          <p className="py-4 text-center text-sm text-neutral-400 dark:text-neutral-500">
            请先在下方配置并保存访问密钥，即可在此管理隧道
          </p>
        ) : tunnelsErr ? (
          <div className="space-y-2 py-3 text-center">
            <p className="text-sm text-red-500">{tunnelsErr}</p>
            <button type="button" onClick={() => void loadTunnels()} className={btnGhostCls}>
              重试
            </button>
          </div>
        ) : tunnels === null ? (
          <p className="py-4 text-center text-sm text-neutral-400">正在加载隧道列表…</p>
        ) : tunnels.length === 0 ? (
          <p className="py-4 text-center text-sm text-neutral-400 dark:text-neutral-500">
            还没有隧道，点击右上角「新建隧道」创建一条
          </p>
        ) : (
          <div className="space-y-2">
            {tunnels.map((t) => {
              const isRunning = running && runningTunnelId === t.id
              const otherRunning = running && runningTunnelId !== null && !isRunning
              return (
                <div
                  key={t.id}
                  className={`rounded-lg border p-2.5 transition-colors duration-150 ${
                    isRunning
                      ? 'border-accent/50 bg-accent-soft'
                      : 'border-neutral-100 dark:border-neutral-800'
                  }`}
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span
                      title={
                        t.banned
                          ? `已封禁${t.statusReason ? `：${t.statusReason}` : ''}`
                          : t.online
                            ? '在线'
                            : '离线'
                      }
                      className={`size-2 shrink-0 rounded-full ${
                        t.banned ? 'bg-red-500' : t.online ? 'bg-emerald-500' : 'bg-neutral-300 dark:bg-neutral-600'
                      }`}
                    />
                    <span className="truncate text-sm font-medium">{t.name}</span>
                    <TypeBadge t={t.tunnelType} />
                    {t.nodeName && (
                      <span className="truncate text-xs text-neutral-400 dark:text-neutral-500">
                        {t.nodeName}
                      </span>
                    )}
                    <span className="ml-auto shrink-0 font-mono text-xs text-neutral-500 dark:text-neutral-400">
                      {t.localPort || '?'} → {t.remote || '未分配'}
                    </span>
                    {isRunning ? (
                      <button
                        type="button"
                        onClick={() => void handleStop()}
                        disabled={busy}
                        className="shrink-0 rounded-md bg-red-500 px-2.5 py-1 text-xs font-medium text-white transition-colors duration-150 hover:bg-red-600 disabled:opacity-50"
                      >
                        停止
                      </button>
                    ) : (
                      <button
                        type="button"
                        onClick={() => void handleStartTunnel(t)}
                        disabled={busy || !ready || otherRunning || t.banned}
                        title={
                          !ready
                            ? '请先下载或指定 frpc'
                            : otherRunning
                              ? '已有穿透在运行，请先停止'
                              : t.banned
                                ? `已封禁${t.statusReason ? `：${t.statusReason}` : ''}`
                                : undefined
                        }
                        className="flex shrink-0 items-center gap-1 rounded-md bg-accent px-2.5 py-1 text-xs font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-40"
                      >
                        <IconPlay className="size-3" />
                        启动
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => void handleDeleteTunnel(t)}
                      disabled={busy || isRunning}
                      title={isRunning ? '运行中的隧道请先停止' : undefined}
                      className={`shrink-0 rounded-md px-2 py-1 text-xs transition-colors duration-150 disabled:opacity-40 ${
                        pendingDeleteId === t.id
                          ? 'bg-red-500 font-medium text-white hover:bg-red-600'
                          : 'text-neutral-400 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10'
                      }`}
                    >
                      {pendingDeleteId === t.id ? '确认删除？' : '删除'}
                    </button>
                  </div>
                </div>
              )
            })}
            <p className="pt-1 text-xs text-neutral-400 dark:text-neutral-500">
              列表每 10 秒自动刷新在线状态；frpc 全局单实例，同一时间只能启动一条隧道
            </p>
          </div>
        )}
      </section>

      {/* 配置卡 */}
      <section className={sectionCls}>
        <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
          <h2 className="flex items-center gap-1.5 text-sm font-semibold">
            <IconLan className="size-4" />
            配置
          </h2>
          {/* 直达入口：常驻显示（不随配置填写状态变化） */}
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={() => openPage('https://www.natfrp.com/user/profile')}
              className="text-xs text-accent transition-colors duration-150 hover:text-accent-hover hover:underline"
            >
              获取访问密钥 ↗
            </button>
            <button
              type="button"
              onClick={() => openPage('https://www.natfrp.com/tunnel/')}
              className="text-xs text-accent transition-colors duration-150 hover:text-accent-hover hover:underline"
            >
              打开隧道列表 ↗
            </button>
          </div>
        </div>
        <div className="space-y-3">
          <div>
            <label className="mb-1.5 block text-sm">
              访问密钥
              {saved && <span className="ml-2 text-xs text-emerald-500">已保存</span>}
            </label>
            <input
              type="password"
              className={inputCls}
              value={accessKey}
              onChange={(e) => {
                setAccessKey(e.target.value)
                setSaved(false)
              }}
              placeholder="natfrp.com 面板「访问密钥」页获取"
            />
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void handleSave()}
              disabled={busy}
              className={btnGhostCls}
            >
              保存配置
            </button>
            {ready ? (
              <span className="truncate text-xs text-neutral-400 dark:text-neutral-500" title={status?.frpcPath}>
                frpc 路径：{status?.frpcPath}
              </span>
            ) : (
              <>
                <button
                  type="button"
                  onClick={() => void handleDownload()}
                  disabled={busy}
                  className="rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
                >
                  自动下载 frpc
                </button>
                <button
                  type="button"
                  onClick={() => void handlePick()}
                  disabled={busy}
                  className={`${btnGhostCls} flex items-center gap-1.5`}
                >
                  <IconFolder className="size-4" />
                  手动指定 frpc.exe
                </button>
              </>
            )}
          </div>
        </div>
      </section>

      {/* 联机步骤引导 */}
      <section className={sectionCls}>
        <h2 className="mb-3 text-sm font-semibold">联机步骤</h2>
        <ol className="list-decimal space-y-2 pl-5 text-sm text-neutral-600 dark:text-neutral-300">
          <li>
            在 <span className="text-accent">natfrp.com</span> 注册并实名后，在上方「隧道列表」点
            <b>新建隧道</b>（TCP 隧道：本地端口填游戏「对局域网开放」时显示的端口，默认 25565）
          </li>
          <li>点击隧道右侧的「启动」开启穿透</li>
          <li>启动游戏实例，进入世界后按 Esc →「对局域网开放」→ 确认端口号与隧道一致</li>
          <li>
            把下方「连接地址」里的 <b>IPv4 地址</b> 发给朋友，朋友在多人游戏中直接连接即可
          </li>
        </ol>
        <p className="mt-2 text-xs text-neutral-400 dark:text-neutral-500">
          公开游戏服务器必须使用专用节点（名称含「多线」）；隧道仅在你启动穿透期间有效
        </p>
      </section>

      {/* 连接地址（自动获取，仅展示 IPv4 直连形式） */}
      {addrs.length > 0 && (
        <section className={sectionCls}>
          <div className="mb-2 flex items-center justify-between">
            <h2 className="text-sm font-semibold">连接地址</h2>
            <span className="text-xs text-neutral-400 dark:text-neutral-500">
              自动获取的 IPv4 连接地址，发给朋友直连
            </span>
          </div>
          <div className="space-y-2">
            {addrs.map((a) => {
              const shown = shownOf(a)
              return (
                <div
                  key={a.address}
                  className="flex items-center gap-2 rounded-lg border border-neutral-100 p-2.5 dark:border-neutral-800"
                >
                  <span className="truncate font-mono text-sm text-accent">{shown}</span>
                  <button
                    type="button"
                    onClick={() => void copyAddr(shown)}
                    className="ml-auto shrink-0 rounded-md border border-neutral-200 px-2 py-0.5 text-xs transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                  >
                    {copied === shown ? '已复制' : '复制'}
                  </button>
                </div>
              )
            })}
          </div>
        </section>
      )}

      {/* frpc 日志 */}
      <section className={sectionCls}>
        <div className="mb-2 flex items-center justify-between">
          <h2 className="text-sm font-semibold">frpc 日志</h2>
          {logs.length > 0 && (
            <button
              type="button"
              onClick={() => setLogs([])}
              className="flex items-center gap-1 text-xs text-neutral-400 transition-colors duration-150 hover:text-accent"
            >
              <IconClose className="size-3" />
              清空
            </button>
          )}
        </div>
        <div
          ref={logRef}
          className="h-52 overflow-y-auto rounded-lg bg-neutral-950 p-3 font-mono text-xs leading-5 text-neutral-300"
        >
          {logs.length === 0 ? (
            <p className="text-neutral-500">暂无日志（启动穿透后显示 frpc 输出）</p>
          ) : (
            logs.map((l, i) => <p key={i} className="whitespace-pre-wrap break-all">{l}</p>)
          )}
        </div>
      </section>

      {/* 新建隧道弹窗 */}
      <CreateTunnelDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={() => {
          setShowCreate(false)
          void loadTunnels()
        }}
      />

      {/* 首次打开的免责声明弹窗（不可点击遮罩 / Esc 关闭，需阅读 5 秒） */}
      <Modal
        open={showDisclaimer}
        onClose={() => {}}
        title="免责声明"
        dismissible={false}
        maxWidth="max-w-md"
      >
        <div className="space-y-3">
          <div className="space-y-2 rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs leading-5 text-amber-700 dark:border-amber-500/30 dark:bg-amber-500/10 dark:text-amber-300">
            <p>
              本启动器仅提供樱花内网穿透（SakuraFrp）的<b>接口调用</b>，
              不提供任何穿透服务；隧道创建规则、节点限制及账号处理均由服务提供方
              SakuraFrp 决定，使用前请阅读并遵守其服务条款。
            </p>
            <p>
              公开游戏服务器（在公共平台宣传、发布服务器列表）必须使用
              <b>专用节点</b>，使用普通节点将被<b>封号且不退款</b>；
              普通节点仅限私密小群联机。
            </p>
            <p>
              请妥善保管访问密钥，泄露可能导致隧道被他人操作。
            </p>
          </div>
          <button
            type="button"
            onClick={ackDisclaimer}
            disabled={disclaimerCountdown > 0}
            className="w-full rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            {disclaimerCountdown > 0
              ? `请仔细阅读（${disclaimerCountdown}s 后可关闭）`
              : '我已阅读并理解'}
          </button>
        </div>
      </Modal>
    </div>
  )
}

/** 新建隧道弹窗：樱花 API 一站式创建（节点列表实时拉取，内置合规警示） */
function CreateTunnelDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean
  onClose: () => void
  onCreated: () => void
}) {
  const [nodes, setNodes] = useState<NatFrpNode[] | null>(null)
  const [nodesErr, setNodesErr] = useState<string | null>(null)
  const [name, setName] = useState('')
  const [tunnelType, setTunnelType] = useState('tcp')
  const [nodeId, setNodeId] = useState<number | null>(null)
  const [localIp, setLocalIp] = useState('127.0.0.1')
  const [localPort, setLocalPort] = useState('25565')
  const [remote, setRemote] = useState('')
  const [note, setNote] = useState('')
  const [ack, setAck] = useState(false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [created, setCreated] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setNodes(null)
    setNodesErr(null)
    setErr(null)
    setCreated(null)
    setAck(false)
    ipc
      .natfrpListNodes()
      .then((list) => {
        setNodes(list)
        // 默认选第一个可用节点
        const ok = list.find((n) => n.canCreate && !n.offline)
        setNodeId(ok?.id ?? null)
      })
      .catch((e) => setNodesErr(String(e).replace(/^Error:\s*/i, '')))
  }, [open])

  const selNode = nodes?.find((n) => n.id === nodeId) ?? null
  const needAck = selNode !== null && !selNode.allowPublicGame
  const isWebType = tunnelType === 'http' || tunnelType === 'https'
  const canSubmit =
    !busy &&
    name.trim() !== '' &&
    nodeId !== null &&
    selNode !== null &&
    selNode.canCreate &&
    !selNode.offline &&
    localPort.trim() !== '' &&
    Number(localPort) > 0 &&
    (!isWebType || remote.trim() !== '') &&
    (!needAck || ack)

  const submit = async () => {
    setBusy(true)
    setErr(null)
    try {
      const id = await ipc.natfrpCreateTunnel({
        name: name.trim(),
        tunnelType,
        nodeId: nodeId!,
        localIp: localIp.trim() || '127.0.0.1',
        localPort: Number(localPort),
        remote: remote.trim() || undefined,
        note: note.trim() || undefined,
      })
      setCreated(`隧道创建成功（ID ${id}）`)
      setTimeout(onCreated, 900)
    } catch (e) {
      setErr(String(e).replace(/^Error:\s*/i, ''))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal open={open} onClose={onClose} title="新建隧道" maxWidth="max-w-lg">
      <div className="space-y-3">
        {created ? (
          <p className="py-6 text-center text-sm text-emerald-500">{created}</p>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="mb-1.5 block text-sm">隧道名称</label>
                <input
                  className={inputCls}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="例如：MC 联机"
                />
              </div>
              <div>
                <label className="mb-1.5 block text-sm">类型</label>
                <select
                  className={inputCls}
                  value={tunnelType}
                  onChange={(e) => setTunnelType(e.target.value)}
                >
                  <option value="tcp">TCP（游戏联机常用）</option>
                  <option value="udp">UDP</option>
                  <option value="http">HTTP（需绑定子域名）</option>
                  <option value="https">HTTPS（需绑定子域名）</option>
                </select>
              </div>
            </div>
            <div>
              <label className="mb-1.5 block text-sm">节点</label>
              {nodesErr ? (
                <p className="text-sm text-red-500">{nodesErr}</p>
              ) : nodes === null ? (
                <p className="text-sm text-neutral-400">正在加载节点列表…</p>
              ) : (
                <select
                  className={inputCls}
                  value={nodeId ?? ''}
                  onChange={(e) => setNodeId(Number(e.target.value))}
                >
                  {nodes.map((n) => (
                    <option key={n.id} value={n.id} disabled={!n.canCreate || n.offline}>
                      {n.name}
                      {n.allowPublicGame ? '（公开游戏专用）' : ''}
                      {n.offline ? '（离线）' : !n.canCreate ? '（满载）' : ''}
                      {n.description ? ` - ${n.description}` : ''}
                    </option>
                  ))}
                </select>
              )}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div>
                <label className="mb-1.5 block text-sm">本地 IP</label>
                <input
                  className={inputCls}
                  value={localIp}
                  onChange={(e) => setLocalIp(e.target.value)}
                  placeholder="127.0.0.1"
                />
              </div>
              <div>
                <label className="mb-1.5 block text-sm">本地端口</label>
                <input
                  className={inputCls}
                  value={localPort}
                  onChange={(e) => setLocalPort(e.target.value.replace(/\D/g, ''))}
                  placeholder="25565"
                />
              </div>
            </div>
            <div>
              <label className="mb-1.5 block text-sm">
                {isWebType ? '绑定子域名（必填）' : '远程端口（留空 = 自动分配）'}
              </label>
              <input
                className={inputCls}
                value={remote}
                onChange={(e) => setRemote(e.target.value)}
                placeholder={isWebType ? '例如 mymc' : '留空自动分配'}
              />
            </div>
            <div>
              <label className="mb-1.5 block text-sm">备注（选填）</label>
              <input
                className={inputCls}
                value={note}
                onChange={(e) => setNote(e.target.value)}
                placeholder="仅自己可见"
              />
            </div>
            <div className="rounded-lg border border-red-200 bg-red-50 p-3 text-xs leading-5 text-red-600 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-300">
              <b>公开游戏服务器警告：</b>
              在公共平台宣传、发布服务器列表的公开服务器，只能使用专用节点（名称含
              「多线」或带「公开游戏专用」标记），使用普通节点将被<b>封号且不退款</b>。
              普通节点仅限私密小群联机。本启动器仅提供接口，不提供穿透服务，
              相关规则由 SakuraFrp 决定。
            </div>
            {needAck && (
              <label className="flex cursor-pointer items-start gap-2 text-xs text-neutral-600 dark:text-neutral-300">
                <input
                  type="checkbox"
                  className="mt-0.5 size-4 accent-[var(--accent)]"
                  checked={ack}
                  onChange={(e) => setAck(e.target.checked)}
                />
                我了解并确认：本次穿透仅用于私密联机，不会将连接方式发布到任何公共空间
              </label>
            )}
            {err && <p className="text-sm text-red-500">{err}</p>}
            <div className="flex justify-end gap-2 pt-1">
              <button type="button" onClick={onClose} className={btnGhostCls}>
                取消
              </button>
              <button
                type="button"
                onClick={() => void submit()}
                disabled={!canSubmit}
                className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
              >
                {busy ? '创建中…' : '创建隧道'}
              </button>
            </div>
          </>
        )}
      </div>
    </Modal>
  )
}
