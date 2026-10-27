// 服务器页：收藏列表 + Server List Ping 实时状态
//
// - 列表持久化于启动器级 servers.json（与实例无关）
// - 每 10 秒轮询全部服务器；单卡可手动刷新；失败时保留最后一次成功状态并标「无法连接」
// - MOTD 为后端统一转换的 § 颜色码字符串，由 MotdText 渲染（像素字体）

import { useCallback, useEffect, useRef, useState } from 'react'
import { ipc } from '../../lib/ipc'
import { useLaunchStore } from '../../stores/launchStore'
import { useUiStore } from '../../stores/uiStore'
import type { ServerEntry, ServerPing } from '../../types/domain'
import type { InstanceSummary } from '../../types/domain'
import {
  IconClose,
  IconMore,
  IconPlay,
  IconPlus,
  IconServer,
} from '../../components/common/Icons'

// ---------------------------------------------------------------------------
// MOTD § 码渲染
// ---------------------------------------------------------------------------

/** 颜色码 → hex */
const COLOR_HEX: Record<string, string> = {
  '0': '#000000',
  '1': '#0000AA',
  '2': '#00AA00',
  '3': '#00AAAA',
  '4': '#AA0000',
  '5': '#AA00AA',
  '6': '#FFAA00',
  '7': '#AAAAAA',
  '8': '#555555',
  '9': '#5555FF',
  a: '#55FF55',
  b: '#55FFFF',
  c: '#FF5555',
  d: '#FF55FF',
  e: '#FFFF55',
  f: '#FFFFFF',
}

interface MotdSeg {
  text: string
  color: string
  bold: boolean
  italic: boolean
  underline: boolean
  strike: boolean
  obf: boolean
}

/** 解析 § 颜色码字符串为带样式段落（色码重置格式，与游戏内语义一致） */
function parseMotd(s: string): MotdSeg[] {
  const segs: MotdSeg[] = []
  let color = '#FFFFFF'
  let bold = false
  let italic = false
  let underline = false
  let strike = false
  let obf = false
  let cur = ''
  const flush = () => {
    if (cur) {
      segs.push({ text: cur, color, bold, italic, underline, strike, obf })
      cur = ''
    }
  }
  for (let i = 0; i < s.length; i++) {
    const ch = s[i]
    if (ch === '§' && i + 1 < s.length) {
      // 十六进制色标记（后端约定）：§#rrggbb
      if (
        s[i + 1] === '#' &&
        i + 7 < s.length &&
        /^#[0-9a-fA-F]{6}$/.test(s.slice(i + 1, i + 8))
      ) {
        flush()
        color = s.slice(i + 1, i + 8).toLowerCase()
        bold = italic = underline = strike = obf = false
        i += 7
        continue
      }
      flush()
      const code = s[i + 1].toLowerCase()
      i++
      if (COLOR_HEX[code]) {
        color = COLOR_HEX[code]
        bold = italic = underline = strike = obf = false
      } else if (code === 'l') bold = true
      else if (code === 'o') italic = true
      else if (code === 'n') underline = true
      else if (code === 'm') strike = true
      else if (code === 'k') obf = true
      else if (code === 'r') {
        color = '#FFFFFF'
        bold = italic = underline = strike = obf = false
      }
    } else {
      cur += ch
    }
  }
  flush()
  return segs
}

/** MOTD 渲染（像素字体；§k 混淆用闪烁动画模拟；含 MiniMessage 标签时走 MM 解析） */
function MotdText({ text }: { text: string }) {
  const segs = mmLooksLike(text) ? mmParse(text) : parseMotd(text)
  return (
    <p className="whitespace-pre-wrap font-mono text-[13px] leading-5 break-all">
      {segs.map((seg, i) => (
        <span
          key={i}
          className={seg.obf ? 'motd-obf' : undefined}
          style={{
            color: seg.color,
            fontWeight: seg.bold ? 700 : 400,
            fontStyle: seg.italic ? 'italic' : undefined,
            textDecoration: [
              seg.underline ? 'underline' : '',
              seg.strike ? 'line-through' : '',
            ]
              .filter(Boolean)
              .join(' ') || undefined,
          }}
        >
          {seg.text}
        </span>
      ))}
    </p>
  )
}

// ---------------------------------------------------------------------------
// MiniMessage 渲染（Adventure 格式：Paper 系服务器常用）
// 支持子集：<bold> <italic> <underlined> <strikethrough> <obfuscated> <reset>
// <color:#hex|named> <#hex> <gradient:#a:#b> <rainbow> <newline>/<br>，
// 支持嵌套与缩写（<b>/<i>/<u>/<st>/<obf>）；hover/click 等未知标签仅剥离标签本身。
// 渐变 / 彩虹按字符插值成逐字符颜色。
// ---------------------------------------------------------------------------

const MM_NAMED: Record<string, string> = COLOR_HEX

/** MiniMessage 命名颜色标签（<yellow> 等）→ hex */
const MM_NAME_TO_HEX: Record<string, string> = {
  black: '#000000',
  dark_blue: '#0000AA',
  dark_green: '#00AA00',
  dark_aqua: '#00AAAA',
  dark_red: '#AA0000',
  dark_purple: '#AA00AA',
  gold: '#FFAA00',
  gray: '#AAAAAA',
  grey: '#AAAAAA',
  dark_gray: '#555555',
  dark_grey: '#555555',
  blue: '#5555FF',
  green: '#55FF55',
  aqua: '#55FFFF',
  red: '#FF5555',
  light_purple: '#FF55FF',
  yellow: '#FFFF55',
  white: '#FFFFFF',
}

function mmLooksLike(s: string): boolean {
  return /<(\/?)(bold|b|italic|i|em|underl?ined|u|strikethrough|st|obf(?:uscated)?|rainbow|gradient|color|c|reset|r|newline|br|hover|click|yellow|gold|aqua|green|red|gray|grey|dark_gray|light_purple|dark_purple|blue|dark_blue|dark_aqua|dark_green|dark_red|dark_purple|black|white|#[0-9a-fA-F]{6})\s*[>:]/.test(
    s,
  )
}

function mmHexColor(v: string, fallback: string): string {
  const t = v.trim()
  if (/^#[0-9a-fA-F]{6}$/.test(t)) return t.toLowerCase()
  return MM_NAME_TO_HEX[t.toLowerCase()] ?? MM_NAMED[t.toLowerCase()] ?? fallback
}

/** HSL 色相 → hex（彩虹用，饱和度/亮度取鲜艳档） */
function mmRainbowHex(hue: number): string {
  const h = ((hue % 360) + 360) % 360
  const s = 1
  const l = 0.5
  const c = (1 - Math.abs(2 * l - 1)) * s
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1))
  const m = l - c / 2
  const rgb =
    h < 60
      ? [c, x, 0]
      : h < 120
        ? [x, c, 0]
        : h < 180
          ? [0, c, x]
          : h < 240
            ? [0, x, c]
            : h < 300
              ? [x, 0, c]
              : [c, 0, x]
  return (
    '#' +
    rgb
      .map((v) => Math.round((v + m) * 255))
      .map((v) => Math.min(255, Math.max(0, v)).toString(16).padStart(2, '0'))
      .join('')
  )
}

interface MmStyle {
  color: string
  bold: boolean
  italic: boolean
  underline: boolean
  strike: boolean
  obf: boolean
  /** 显式设置过颜色（渐变区域内嵌套颜色标签优先于插值） */
  explicit: boolean
}

const MM_DEFAULT: MmStyle = {
  color: '#FFFFFF',
  bold: false,
  italic: false,
  underline: false,
  strike: false,
  obf: false,
  explicit: false,
}

/** 找到与 src[from..] 处同名开标签匹配的关闭标签位置（计同名嵌套深度） */
function mmFindClose(src: string, from: number, name: string): number {
  const openRe = new RegExp(`<${name}[\\s>:]`, 'gi')
  const closeRe = new RegExp(`</${name}\\s*>`, 'gi')
  let depth = 1
  openRe.lastIndex = from
  closeRe.lastIndex = from
  for (;;) {
    const o = openRe.exec(src)
    const c = closeRe.exec(src)
    if (!c) return -1
    if (o && o.index < c.index) {
      depth++
      openRe.lastIndex = o.index + 1
    } else {
      depth--
      if (depth === 0) return c.index
      closeRe.lastIndex = c.index + 1
    }
  }
}

/** 递归渲染：colorFn 非空时逐字符取色（渐变 / 彩虹区域） */
function mmRender(
  src: string,
  styleIn: MmStyle,
  colorFn: ((i: number) => string) | null,
  out: MotdSeg[],
): void {
  let style = { ...styleIn }
  const stack: MmStyle[] = []
  let buf = ''
  let ci = 0 // 渐变插值用的字符计数（不含标签本身）
  const flush = () => {
    if (buf) {
      out.push({
        text: buf,
        color: !style.explicit && colorFn ? colorFn(ci - buf.length) : style.color,
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
        strike: style.strike,
        obf: style.obf,
      })
      buf = ''
    }
  }
  let i = 0
  while (i < src.length) {
    // § 码兼容：真实服务器的 MOTD 可能是 § 码与 MiniMessage 混合格式
    if (src[i] === '§' && i + 1 < src.length) {
      flush()
      const code = src[i + 1].toLowerCase()
      i += 2
      if (COLOR_HEX[code]) {
        style.color = COLOR_HEX[code]
        style.explicit = true
      } else if (code === 'l') style.bold = true
      else if (code === 'o') style.italic = true
      else if (code === 'n') style.underline = true
      else if (code === 'm') style.strike = true
      else if (code === 'k') style.obf = true
      else if (code === 'r') style = { ...MM_DEFAULT }
      continue
    }
    if (src.startsWith('\\<', i)) {
      buf += '<'
      i += 2
      ci++
      if (colorFn) flush()
      continue
    }
    if (src[i] !== '<') {
      buf += src[i]
      i++
      ci++
      // 渐变 / 彩虹区域逐字符 flush：每个字符独立插值取色
      if (colorFn) flush()
      continue
    }
    const close = src.indexOf('>', i)
    if (close === -1) {
      buf += src[i]
      i++
      ci++
      continue
    }
    const raw = src.slice(i + 1, close)
    i = close + 1
    const closing = raw.startsWith('/')
    const body = closing ? raw.slice(1) : raw
    const colon = body.indexOf(':')
    let name = (colon === -1 ? body : body.slice(0, colon)).trim().toLowerCase()
    let args = colon === -1 ? '' : body.slice(colon + 1)
    // <#RRGGBB> 简写：完整 hex 在 name 中，归一到 '#' + args 形态
    if (name.startsWith('#')) {
      args = name
      name = '#'
    }
    switch (name) {
      case 'bold':
      case 'b': {
        flush()
        if (closing) {
          style = stack.pop() ?? style
        } else {
          stack.push({ ...style })
          style.bold = true
        }
        break
      }
      case 'italic':
      case 'i':
      case 'em': {
        flush()
        if (closing) {
          style = stack.pop() ?? style
        } else {
          stack.push({ ...style })
          style.italic = true
        }
        break
      }
      case 'underlined':
      case 'u': {
        flush()
        if (closing) {
          style = stack.pop() ?? style
        } else {
          stack.push({ ...style })
          style.underline = true
        }
        break
      }
      case 'strikethrough':
      case 'st': {
        flush()
        if (closing) {
          style = stack.pop() ?? style
        } else {
          stack.push({ ...style })
          style.strike = true
        }
        break
      }
      case 'obfuscated':
      case 'obf': {
        flush()
        if (closing) {
          style = stack.pop() ?? style
        } else {
          stack.push({ ...style })
          style.obf = true
        }
        break
      }
      case 'color':
      case 'c':
      case '#': {
        flush()
        if (closing) {
          style = stack.pop() ?? style
        } else {
          stack.push({ ...style })
          style.color = mmHexColor(args, style.color)
          style.explicit = true
        }
        break
      }
      case 'reset':
      case 'r': {
        flush()
        style = { ...MM_DEFAULT }
        stack.length = 0
        break
      }
      case 'newline':
      case 'br': {
        flush()
        buf = '\n'
        flush()
        ci++
        break
      }
      case 'gradient': {
        flush()
        if (closing) break
        // 从当前位置（开标签之后）找匹配关闭标签，避免把自身计入嵌套
        const end = mmFindClose(src, i, 'gradient')
        if (end === -1) break
        const inner = src.slice(i, end)
        i = end + `</gradient>`.length
        // 色标支持 hex 与命名颜色（<gradient:yellow:gold>），忽略 phase 等非颜色参数
        const stops = args
          .split(':')
          .map((a) => a.trim())
          .filter((a) => /^#[0-9a-fA-F]{6}$/.test(a) || MM_NAME_TO_HEX[a.toLowerCase()])
          .map((a) => mmHexColor(a, '#FFFFFF'))
        const from = stops[0] ?? '#FFFFFF'
        const to = stops[stops.length - 1] ?? from
        const len = Math.max(1, inner.replace(/<[^>]*>/g, '').length)
        const colorFnG = (ci0: number) => {
          const t = stops.length >= 3 ? (ci0 / len) * (stops.length - 1) : ci0 / len
          const segIdx = Math.min(Math.floor(t), stops.length >= 3 ? stops.length - 2 : 0)
          const local = stops.length >= 3 ? t - segIdx : t
          const a = stops.length >= 3 ? stops[segIdx] : from
          const b = stops.length >= 3 ? stops[segIdx + 1] ?? to : to
          const pa = parseInt(a.slice(1), 16)
          const pb = parseInt(b.slice(1), 16)
          const mix = (sh: number) => {
            const av = (pa >> sh) & 0xff
            const bv = (pb >> sh) & 0xff
            return Math.round(av + (bv - av) * local)
          }
          return `#${[16, 8, 0].map(mix).map((v) => v.toString(16).padStart(2, '0')).join('')}`
        }
        mmRender(inner, { ...style }, colorFnG, out)
        break
      }
      case 'rainbow': {
        flush()
        if (closing) break
        const end = mmFindClose(src, i, 'rainbow')
        if (end === -1) break
        const inner = src.slice(i, end)
        i = end + `</rainbow>`.length
        const phase = parseFloat(args.split(':')[0] ?? '')
        const phaseOff = isNaN(phase) ? 0 : (phase * 360) % 360
        const len = Math.max(1, inner.replace(/<[^>]*>/g, '').length)
        const colorFnR = (ci0: number) =>
          mmRainbowHex(phaseOff + (ci0 / Math.max(1, len)) * 360)
        mmRender(inner, { ...style }, colorFnR, out)
        break
      }
      default: {
        // 命名颜色标签（<yellow> 等）：开 / 闭与 color 标签同语义
        if (MM_NAME_TO_HEX[name]) {
          flush()
          if (closing) {
            style = stack.pop() ?? style
          } else {
            stack.push({ ...style })
            style.color = MM_NAME_TO_HEX[name]
            style.explicit = true
          }
        }
        // 其余（hover / click / 未知标签）：剥离标签本身，内容照常渲染
        break
      }
    }
  }
  flush()
}

/** MiniMessage → 样式段落（仅在含 MM 标签时被调用） */
function mmParse(s: string): MotdSeg[] {
  const out: MotdSeg[] = []
  mmRender(s.replace(/\r\n/g, '\n'), { ...MM_DEFAULT }, null, out)
  return out.length > 0 ? out : parseMotd(s)
}

// ---------------------------------------------------------------------------
// 卡片状态
// ---------------------------------------------------------------------------

interface CardState {
  status: 'pinging' | 'ok' | 'fail'
  ping: ServerPing | null
  error: string | null
}

const emptyCard: CardState = { status: 'pinging', ping: null, error: null }

/** 延迟配色 */
function latencyDot(ms: number) {
  if (ms < 80) return 'bg-emerald-500'
  if (ms < 250) return 'bg-amber-500'
  return 'bg-red-500'
}

function formatPlayers(n: number) {
  return n.toLocaleString('zh-CN')
}

export function ServersView() {
  const [entries, setEntries] = useState<ServerEntry[] | null>(null)
  const [cards, setCards] = useState<Record<string, CardState>>({})
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const [editing, setEditing] = useState<ServerEntry | null>(null)
  const [adding, setAdding] = useState(false)
  // 加入服务器弹窗：joining = 弹窗开关；joinPreselect = 从卡片菜单进入时预选的服务器
  const [joining, setJoining] = useState(false)
  const [joinPreselect, setJoinPreselect] = useState<ServerEntry | null>(null)
  // 导入到实例弹窗
  const [importing, setImporting] = useState(false)
  // 玩家列表弹窗
  const [playerListFor, setPlayerListFor] = useState<ServerEntry | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  // 列表加载 + 手动全量刷新
  const refreshList = useCallback(async () => {
    try {
      const list = await ipc.listServers()
      setEntries(list)
      // 并发 ping 全部（各卡独立展示，失败不影响其他卡）
      list.forEach(async (entry) => {
        setCards((c) => ({
          ...c,
          [entry.id]: { ...(c[entry.id] ?? emptyCard), status: 'pinging' },
        }))
        try {
          const ping = await ipc.pingServer(entry.address)
          setCards((c) => ({ ...c, [entry.id]: { status: 'ok', ping, error: null } }))
        } catch (e) {
          setCards((c) => ({
            ...c,
            [entry.id]: { status: 'fail', ping: c[entry.id]?.ping ?? null, error: String(e) },
          }))
        }
      })
    } catch {
      /* 列表加载失败：保持现状 */
    }
  }, [])

  useEffect(() => {
    void refreshList()
    // 10 秒轮询
    const timer = setInterval(() => void refreshList(), 10_000)
    return () => clearInterval(timer)
  }, [refreshList])

  // 点击菜单外部收起
  useEffect(() => {
    if (!menuFor) return
    const onDocClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuFor(null)
      }
    }
    document.addEventListener('mousedown', onDocClick)
    return () => document.removeEventListener('mousedown', onDocClick)
  }, [menuFor])

  const handleRemove = async (entry: ServerEntry) => {
    setMenuFor(null)
    if (!window.confirm(`确定删除服务器「${entry.name}」吗？`)) return
    try {
      await ipc.removeServer(entry.id)
      await refreshList()
    } catch (e) {
      window.alert(`删除失败：${e}`)
    }
  }

  return (
    <div className="flex h-full flex-col">
      {/* 头部 */}
      <div className="flex flex-wrap items-center gap-3 px-6 pt-6">
        <h1 className="text-lg font-semibold">服务器</h1>
        {entries && (
          <span className="text-xs text-neutral-400 dark:text-neutral-500">
            {entries.length} 个收藏 · 每 10 秒自动刷新
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={() => void refreshList()}
            className="rounded-lg border border-neutral-200 px-3 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            刷新
          </button>
          <button
            type="button"
            onClick={() => {
              setJoinPreselect(null)
              setJoining(true)
            }}
            disabled={entries === null || entries.length === 0}
            title="选择服务器与实例，启动后直连"
            className="rounded-lg border border-neutral-200 px-3 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            加入服务器
          </button>
          <button
            type="button"
            onClick={() => setImporting(true)}
            disabled={entries === null || entries.length === 0}
            title="把收藏的全部服务器写入某个实例的服务器列表"
            className="rounded-lg border border-neutral-200 px-3 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            导入到实例
          </button>
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="flex items-center gap-1.5 rounded-lg bg-accent px-3.5 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
          >
            <IconPlus className="size-4" />
            添加服务器
          </button>
        </div>
      </div>
      <p className="px-6 pt-2 text-xs text-neutral-400 dark:text-neutral-500">
        通过 Server List Ping 协议实时查询头像、MOTD、版本与在线人数（支持 SRV 记录解析与 Forge 服务器标记）
      </p>

      <main className="min-h-0 flex-1 overflow-y-auto p-6 pt-4">
        {entries === null ? (
          <p className="py-10 text-center text-sm text-neutral-400">正在加载…</p>
        ) : entries.length === 0 ? (
          <div className="flex flex-col items-center gap-3 rounded-xl border border-neutral-200 bg-white py-14 dark:border-neutral-800 dark:bg-neutral-900">
            <IconServer className="size-10 opacity-30" />
            <p className="text-sm text-neutral-400 dark:text-neutral-500">
              还没有收藏服务器，点击右上角「添加服务器」开始
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {entries.map((entry) => {
              const card = cards[entry.id] ?? emptyCard
              const ping = card.ping
              return (
                <div
                  key={entry.id}
                  className={`flex items-center gap-3 rounded-xl border bg-white p-3 transition-colors duration-150 dark:bg-neutral-900 ${
                    card.status === 'fail'
                      ? 'border-neutral-200 opacity-60 dark:border-neutral-800'
                      : 'border-neutral-200 hover:border-accent/50 dark:border-neutral-800'
                  }`}
                >
                  {/* 头像 */}
                  {ping?.favicon ? (
                    <img
                      src={ping.favicon}
                      alt=""
                      draggable={false}
                      className="size-16 shrink-0 rounded-lg [image-rendering:pixelated]"
                    />
                  ) : (
                    <div className="flex size-16 shrink-0 items-center justify-center rounded-lg bg-neutral-100 dark:bg-neutral-800">
                      <IconServer className="size-7 text-neutral-400" />
                    </div>
                  )}

                  {/* 名称 + MOTD */}
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-semibold" title={entry.name}>
                      {entry.name}
                    </p>
                    {card.status === 'ok' && ping ? (
                      <MotdText text={ping.motd || '§7（无 MOTD）'} />
                    ) : card.status === 'fail' ? (
                      <p className="truncate text-xs text-red-400" title={card.error ?? undefined}>
                        {card.error ?? '无法连接'}
                      </p>
                    ) : (
                      <p className="truncate text-xs text-neutral-400">正在查询…</p>
                    )}
                    <p className="truncate text-xs text-neutral-400 dark:text-neutral-500" title={entry.address}>
                      {entry.address}
                    </p>
                  </div>

                  {/* 版本 / 在线人数 / 延迟 */}
                  <div className="shrink-0 space-y-1 text-right">
                    {card.status === 'ok' && ping ? (
                      <>
                        <p className="text-xs text-neutral-500 dark:text-neutral-400">
                          {ping.versionName}
                          {ping.forge && (
                            <span className="ml-1.5 rounded bg-orange-100 px-1 py-0.5 text-[10px] text-orange-600 dark:bg-orange-500/15 dark:text-orange-300">
                              Forge
                            </span>
                          )}
                        </p>
                        <p className="text-xs text-neutral-500 dark:text-neutral-400">
                          玩家 {formatPlayers(ping.playersOnline)} / {formatPlayers(ping.playersMax)}
                        </p>
                        <p className="flex items-center justify-end gap-1.5 text-xs text-neutral-500 dark:text-neutral-400">
                          <span className={`size-1.5 rounded-full ${latencyDot(ping.latencyMs)}`} />
                          {ping.latencyMs} ms
                        </p>
                      </>
                    ) : card.status === 'fail' ? (
                      <p className="text-xs text-neutral-400">无法连接</p>
                    ) : (
                      <p className="text-xs text-neutral-400">查询中</p>
                    )}
                  </div>

                  {/* 加入 + 更多菜单 */}
                  <div className="flex shrink-0 items-center gap-1.5">
                    <button
                      type="button"
                      title="选择实例并启动后直连该服务器"
                      onClick={() => {
                        setJoinPreselect(entry)
                        setJoining(true)
                      }}
                      className="flex items-center gap-1.5 rounded-lg border border-neutral-200 px-3 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:border-accent hover:text-accent dark:border-neutral-700 dark:text-neutral-300"
                    >
                      <IconPlay className="size-3.5" />
                      加入
                    </button>
                    <div className="relative" ref={menuFor === entry.id ? menuRef : undefined}>
                      <button
                        type="button"
                        onClick={() => setMenuFor(menuFor === entry.id ? null : entry.id)}
                        className="flex size-8 items-center justify-center rounded-lg text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800"
                      >
                        <IconMore className="size-4" />
                      </button>
                      {menuFor === entry.id && (
                        <div className="absolute right-0 top-9 z-30 w-36 rounded-xl border border-neutral-200 bg-white py-1 shadow-lg animate-[fade-in_120ms_ease-out] dark:border-neutral-700 dark:bg-neutral-800">
                          <button
                            type="button"
                            className="flex w-full items-center px-3 py-2 text-left text-sm transition-colors duration-150 hover:bg-neutral-100 dark:hover:bg-neutral-700"
                            onClick={() => {
                              setMenuFor(null)
                              setPlayerListFor(entry)
                            }}
                          >
                            玩家列表
                          </button>
                          <button
                            type="button"
                            className="flex w-full items-center px-3 py-2 text-left text-sm transition-colors duration-150 hover:bg-neutral-100 dark:hover:bg-neutral-700"
                            onClick={() => {
                              setMenuFor(null)
                              setEditing(entry)
                            }}
                          >
                            编辑
                          </button>
                          <button
                            type="button"
                            className="flex w-full items-center px-3 py-2 text-left text-sm text-red-500 transition-colors duration-150 hover:bg-red-50 dark:hover:bg-red-500/10"
                            onClick={() => void handleRemove(entry)}
                          >
                            删除
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </main>

      {/* 添加 / 编辑弹窗 */}
      {(adding || editing) && (
        <ServerDialog
          initial={editing}
          onClose={() => {
            setAdding(false)
            setEditing(null)
          }}
          onSaved={() => {
            setAdding(false)
            setEditing(null)
            void refreshList()
          }}
        />
      )}

      {/* 加入服务器弹窗 */}
      {joining && (
        <JoinServerDialog
          preselect={joinPreselect}
          entries={entries ?? []}
          onClose={() => setJoining(false)}
        />
      )}

      {/* 导入到实例弹窗 */}
      {importing && (
        <ImportDialog
          count={entries?.length ?? 0}
          onClose={() => setImporting(false)}
        />
      )}

      {/* 玩家列表弹窗 */}
      {playerListFor && (
        <PlayerListDialog entry={playerListFor} onClose={() => setPlayerListFor(null)} />
      )}
    </div>
  )
}

/** 添加 / 编辑服务器弹窗 */
function ServerDialog({
  initial,
  onClose,
  onSaved,
}: {
  initial: ServerEntry | null
  onClose: () => void
  onSaved: () => void
}) {
  const [name, setName] = useState(initial?.name ?? '')
  const [address, setAddress] = useState(initial?.address ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleConfirm = async () => {
    setBusy(true)
    setError(null)
    try {
      if (initial) {
        await ipc.updateServer(initial.id, name.trim() || address.trim(), address.trim())
      } else {
        await ipc.addServer(name.trim() || null, address.trim())
      }
      onSaved()
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className="absolute inset-0 z-40 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="w-[24rem] animate-[modal-in_150ms_ease-out] rounded-xl border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-800 dark:bg-neutral-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">{initial ? '编辑服务器' : '添加服务器'}</h3>
          <button
            type="button"
            onClick={onClose}
            className="flex size-7 items-center justify-center rounded-md text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800"
          >
            <IconClose className="size-4" />
          </button>
        </div>
        <div className="mt-4 space-y-3">
          <div>
            <label className="mb-1.5 block text-sm">名称（可选，缺省用地址）</label>
            <input
              className="w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="例如：Hypixel"
              maxLength={32}
            />
          </div>
          <div>
            <label className="mb-1.5 block text-sm">服务器地址</label>
            <input
              className="w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              placeholder="host 或 host:端口（支持 SRV 域名）"
              onKeyDown={(e) => {
                if (e.key === 'Enter') void handleConfirm()
              }}
            />
          </div>
          {error && <p className="text-xs text-red-500">{error}</p>}
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void handleConfirm()}
            disabled={busy || address.trim() === ''}
            className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            {busy ? '保存中…' : initial ? '保存' : '添加'}
          </button>
        </div>
      </div>
    </div>
  )
}

/** 加入服务器弹窗：选择服务器 + 实例 → 启动实例并直连 */
function JoinServerDialog({
  preselect,
  entries,
  onClose,
}: {
  preselect: ServerEntry | null
  entries: ServerEntry[]
  onClose: () => void
}) {
  const [serverId, setServerId] = useState(preselect?.id ?? entries[0]?.id ?? '')
  const [instanceId, setInstanceId] = useState('')
  const [instances, setInstances] = useState<InstanceSummary[] | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const setActive = useLaunchStore((s) => s.setActive)
  const setCurrentView = useUiStore((s) => s.setCurrentView)

  useEffect(() => {
    void ipc
      .listInstances()
      .then((list) => {
        setInstances(list)
        if (list.length > 0) setInstanceId(list[0].id)
      })
      .catch((e) => setError(String(e)))
  }, [])

  const server = entries.find((s) => s.id === serverId) ?? null
  const handleJoin = async () => {
    if (!server || !instanceId) return
    setBusy(true)
    setError(null)
    try {
      await ipc.launchInstance(instanceId, server.address)
      // 切到日志页跟踪启动进度
      setActive(instanceId)
      setCurrentView('log')
      onClose()
    } catch (e) {
      setError(String(e))
      setBusy(false)
    }
  }

  return (
    <div
      className="absolute inset-0 z-40 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="w-[24rem] animate-[modal-in_150ms_ease-out] rounded-xl border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-800 dark:bg-neutral-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">加入服务器</h3>
          <button
            type="button"
            onClick={onClose}
            className="flex size-7 items-center justify-center rounded-md text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800"
          >
            <IconClose className="size-4" />
          </button>
        </div>
        <div className="mt-4 space-y-3">
          <div>
            <label className="mb-1.5 block text-sm">服务器</label>
            <select
              className="w-full truncate rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
              value={serverId}
              onChange={(e) => setServerId(e.target.value)}
            >
              {entries.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}（{s.address}）
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="mb-1.5 block text-sm">使用实例</label>
            <select
              className="w-full truncate rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
              value={instanceId}
              onChange={(e) => setInstanceId(e.target.value)}
            >
              {instances === null && <option value="">加载中…</option>}
              {instances?.length === 0 && <option value="">暂无实例</option>}
              {instances?.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name}（{i.mcVersion}
                  {i.loaderType !== 'vanilla' ? ` · ${i.loaderType}` : ''}）
                </option>
              ))}
            </select>
            <p className="mt-1 text-xs text-neutral-400 dark:text-neutral-500">
              请确保实例版本与服务器兼容；启动后将自动直连该服务器
            </p>
          </div>
          {error && <p className="text-xs text-red-500">{error}</p>}
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void handleJoin()}
            disabled={busy || !server || !instanceId}
            className="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            <IconPlay className="size-3.5" />
            {busy ? '启动中…' : '加入'}
          </button>
        </div>
      </div>
    </div>
  )
}

/** 导入到实例弹窗：把收藏服务器写入实例的服务器列表（servers.dat，合并去重） */
function ImportDialog({ count, onClose }: { count: number; onClose: () => void }) {
  const [instanceId, setInstanceId] = useState('')
  const [instances, setInstances] = useState<InstanceSummary[] | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [result, setResult] = useState<{ imported: number; skipped: number } | null>(null)

  useEffect(() => {
    void ipc
      .listInstances()
      .then((list) => {
        setInstances(list)
        if (list.length > 0) setInstanceId(list[0].id)
      })
      .catch((e) => setError(String(e)))
  }, [])

  const handleImport = async () => {
    if (!instanceId) return
    setBusy(true)
    setError(null)
    try {
      setResult(await ipc.exportServersToInstance(instanceId))
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className="absolute inset-0 z-40 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="w-[24rem] animate-[modal-in_150ms_ease-out] rounded-xl border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-800 dark:bg-neutral-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">导入到实例</h3>
          <button
            type="button"
            onClick={onClose}
            className="flex size-7 items-center justify-center rounded-md text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800"
          >
            <IconClose className="size-4" />
          </button>
        </div>
        {result ? (
          <div className="mt-4 space-y-3">
            <p className="text-sm text-emerald-600 dark:text-emerald-400">
              导入完成：新增 {result.imported} 条
              {result.skipped > 0 ? `，跳过 ${result.skipped} 条重复` : ''}。
            </p>
            <p className="text-xs text-neutral-400 dark:text-neutral-500">
              启动该实例进入多人游戏菜单即可看到导入的服务器。
            </p>
            <div className="flex justify-end">
              <button
                type="button"
                onClick={onClose}
                className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover"
              >
                完成
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="mt-4 space-y-3">
              <div>
                <label className="mb-1.5 block text-sm">目标实例</label>
                <select
                  className="w-full truncate rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
                  value={instanceId}
                  onChange={(e) => setInstanceId(e.target.value)}
                >
                  {instances === null && <option value="">加载中…</option>}
                  {instances?.length === 0 && <option value="">暂无实例</option>}
                  {instances?.map((i) => (
                    <option key={i.id} value={i.id}>
                      {i.name}（{i.mcVersion}
                      {i.loaderType !== 'vanilla' ? ` · ${i.loaderType}` : ''}）
                    </option>
                  ))}
                </select>
                <p className="mt-1.5 text-xs leading-5 text-neutral-400 dark:text-neutral-500">
                  把收藏的 {count} 个服务器写入该实例的服务器列表；已存在的同地址服务器会自动跳过。
                  实例正在运行时请先关闭，避免游戏覆盖写入。
                </p>
              </div>
              {error && <p className="text-xs text-red-500">{error}</p>}
            </div>
            <div className="mt-5 flex justify-end gap-2">
              <button
                type="button"
                onClick={onClose}
                className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                取消
              </button>
              <button
                type="button"
                onClick={() => void handleImport()}
                disabled={busy || !instanceId || instances?.length === 0}
                className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
              >
                {busy ? '导入中…' : '导入'}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}

/** 玩家列表弹窗：实时 ping 展示在线玩家（players.sample；部分服务器不提供） */
function PlayerListDialog({ entry, onClose }: { entry: ServerEntry; onClose: () => void }) {
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [ping, setPing] = useState<ServerPing | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setPing(await ipc.pingServer(entry.address))
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/i, ''))
      setPing(null)
    } finally {
      setLoading(false)
    }
  }, [entry.address])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const sample = ping?.playerSample ?? []

  return (
    <div
      className="absolute inset-0 z-40 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="w-[22rem] animate-[modal-in_150ms_ease-out] rounded-xl border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-800 dark:bg-neutral-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <div className="min-w-0">
            <h3 className="truncate text-sm font-semibold">在线玩家 · {entry.name}</h3>
            {ping && (
              <p className="mt-0.5 text-xs text-neutral-400 dark:text-neutral-500">
                {ping.playersOnline} / {ping.playersMax} 在线
              </p>
            )}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="flex size-7 items-center justify-center rounded-md text-neutral-400 transition-colors duration-150 hover:bg-neutral-100 hover:text-neutral-600 dark:hover:bg-neutral-800"
          >
            <IconClose className="size-4" />
          </button>
        </div>

        <div className="mt-4 max-h-72 min-h-24 overflow-y-auto">
          {loading ? (
            <p className="py-8 text-center text-sm text-neutral-400">正在查询…</p>
          ) : error ? (
            <p className="py-4 text-center text-sm text-red-500">{error}</p>
          ) : sample.length > 0 ? (
            <div className="space-y-1.5">
              {sample.map((name) => (
                <div
                  key={name}
                  className="flex items-center gap-2.5 rounded-lg border border-neutral-100 px-3 py-2 dark:border-neutral-800"
                >
                  <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-accent-soft text-xs font-semibold text-accent">
                    {name.charAt(0).toUpperCase()}
                  </span>
                  <span className="truncate text-sm">{name}</span>
                </div>
              ))}
            </div>
          ) : (
            <div className="py-6 text-center">
              <p className="text-sm text-neutral-400 dark:text-neutral-500">
                该服务器未公开在线玩家名单
              </p>
              <p className="mt-1 text-xs text-neutral-400 dark:text-neutral-500">
                部分服务器出于隐私考虑隐藏了玩家信息，无法获取
              </p>
            </div>
          )}
        </div>

        <div className="mt-4 flex items-center justify-between">
          <span className="text-xs text-neutral-400 dark:text-neutral-500">
            仅显示服务器主动公开的玩家，可能不完整
          </span>
          <button
            type="button"
            onClick={() => void refresh()}
            disabled={loading}
            className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            {loading ? '刷新中…' : '刷新'}
          </button>
        </div>
      </div>
    </div>
  )
}
