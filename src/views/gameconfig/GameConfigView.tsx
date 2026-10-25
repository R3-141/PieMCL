// 游戏配置页：启动器全局配置模板（PCL 式）
//
// 页面直接编辑一份全局配置（不绑定实例）→「保存」持久化到启动器数据目录
// →「应用到实例」弹窗多选后批量写入各实例 options.txt（版本键名 / 键码自动转换）。
// 模板语义：未设置的项不同步（显示游戏默认值 + 「默认」标记，可一键重置），
// 应用时只写入明确设置过的项，其余 options.txt 内容零接触。

import { useCallback, useEffect, useMemo, useState } from 'react'
import { ipc, onInstancesChanged } from '../../lib/ipc'
import type { GameConfigTemplate, InstanceSummary } from '../../types/domain'
import { IconClose, IconConfig } from '../../components/common/Icons'

/** 键位定义：action id → 中文名 + 默认符号（与 Rust KEY_SPECS 对应） */
const KEY_DEFS: ReadonlyArray<{ action: string; label: string; def: string }> = [
  { action: 'key.forward', label: '前进', def: 'key.keyboard.w' },
  { action: 'key.left', label: '左移', def: 'key.keyboard.a' },
  { action: 'key.back', label: '后退', def: 'key.keyboard.s' },
  { action: 'key.right', label: '右移', def: 'key.keyboard.d' },
  { action: 'key.jump', label: '跳跃', def: 'key.keyboard.space' },
  { action: 'key.sneak', label: '潜行', def: 'key.keyboard.left.shift' },
  { action: 'key.sprint', label: '冲刺', def: 'key.keyboard.left.control' },
  { action: 'key.inventory', label: '背包', def: 'key.keyboard.e' },
  { action: 'key.drop', label: '丢弃物品', def: 'key.keyboard.q' },
  { action: 'key.chat', label: '聊天', def: 'key.keyboard.t' },
  { action: 'key.command', label: '命令', def: 'key.keyboard.slash' },
  { action: 'key.swapOffhand', label: '副手交换', def: 'key.keyboard.f' },
  { action: 'key.playerlist', label: '玩家列表', def: 'key.keyboard.tab' },
  { action: 'key.fullscreen', label: '全屏切换', def: 'key.keyboard.f11' },
  { action: 'key.screenshot', label: '截图', def: 'key.keyboard.f2' },
  { action: 'key.togglePerspective', label: '切换玩家视角', def: 'key.keyboard.f5' },
  { action: 'key.toggleDebugF3', label: '调试屏幕', def: 'key.keyboard.f3' },
  { action: 'key.attack', label: '攻击', def: 'key.mouse.left' },
  { action: 'key.use', label: '使用 / 放置', def: 'key.mouse.right' },
  { action: 'key.pickItem', label: '选取方块', def: 'key.mouse.middle' },
  ...Array.from({ length: 9 }, (_, i) => ({
    action: `key.hotbar.${i + 1}`,
    label: `快捷栏 ${i + 1}`,
    def: `key.keyboard.${i + 1}`,
  })),
]

/** 可绑定符号 → 显示名 */
const SYMBOL_LABELS: Readonly<Record<string, string>> = {
  ...Object.fromEntries(
    'abcdefghijklmnopqrstuvwxyz'.split('').map((c) => [`key.keyboard.${c}`, c.toUpperCase()]),
  ),
  ...Object.fromEntries(
    '1234567890'.split('').map((d) => [`key.keyboard.${d}`, d]),
  ),
  ...Object.fromEntries(
    Array.from({ length: 12 }, (_, i) => [`key.keyboard.f${i + 1}`, `F${i + 1}`]),
  ),
  'key.keyboard.space': '空格',
  'key.keyboard.left.shift': '左 Shift',
  'key.keyboard.right.shift': '右 Shift',
  'key.keyboard.left.control': '左 Ctrl',
  'key.keyboard.right.control': '右 Ctrl',
  'key.keyboard.left.alt': '左 Alt',
  'key.keyboard.right.alt': '右 Alt',
  'key.keyboard.up': '↑',
  'key.keyboard.down': '↓',
  'key.keyboard.left': '←',
  'key.keyboard.right': '→',
  'key.keyboard.enter': 'Enter',
  'key.keyboard.escape': 'Esc',
  'key.keyboard.tab': 'Tab',
  'key.keyboard.backspace': '退格',
  'key.keyboard.caps.lock': 'CapsLock',
  'key.keyboard.insert': 'Insert',
  'key.keyboard.delete': 'Delete',
  'key.keyboard.home': 'Home',
  'key.keyboard.end': 'End',
  'key.keyboard.page.up': 'PgUp',
  'key.keyboard.page.down': 'PgDn',
  'key.keyboard.minus': '-',
  'key.keyboard.equal': '=',
  'key.keyboard.left.bracket': '[',
  'key.keyboard.right.bracket': ']',
  'key.keyboard.backslash': '\\',
  'key.keyboard.semicolon': ';',
  'key.keyboard.apostrophe': "'",
  'key.keyboard.grave': '`',
  'key.keyboard.comma': ',',
  'key.keyboard.period': '.',
  'key.keyboard.slash': '/',
  'key.mouse.left': '鼠标左键',
  'key.mouse.right': '鼠标右键',
  'key.mouse.middle': '鼠标中键',
}

const symbolLabel = (v: string) => SYMBOL_LABELS[v] ?? v

/** KeyboardEvent.code → MC 键位符号（与 Rust 符号表对应） */
const CODE_SYMBOLS: Readonly<Record<string, string>> = {
  ...Object.fromEntries(
    'abcdefghijklmnopqrstuvwxyz'.split('').map((c) => [`Key${c.toUpperCase()}`, `key.keyboard.${c}`]),
  ),
  ...Object.fromEntries(
    '1234567890'.split('').map((d) => [`Digit${d}`, `key.keyboard.${d}`]),
  ),
  ...Object.fromEntries(
    Array.from({ length: 12 }, (_, i) => [`F${i + 1}`, `key.keyboard.f${i + 1}`]),
  ),
  Space: 'key.keyboard.space',
  Tab: 'key.keyboard.tab',
  Enter: 'key.keyboard.enter',
  Backspace: 'key.keyboard.backspace',
  CapsLock: 'key.keyboard.caps.lock',
  Escape: 'key.keyboard.escape',
  ShiftLeft: 'key.keyboard.left.shift',
  ShiftRight: 'key.keyboard.right.shift',
  ControlLeft: 'key.keyboard.left.control',
  ControlRight: 'key.keyboard.right.control',
  AltLeft: 'key.keyboard.left.alt',
  AltRight: 'key.keyboard.right.alt',
  ArrowUp: 'key.keyboard.up',
  ArrowDown: 'key.keyboard.down',
  ArrowLeft: 'key.keyboard.left',
  ArrowRight: 'key.keyboard.right',
  Insert: 'key.keyboard.insert',
  Delete: 'key.keyboard.delete',
  Home: 'key.keyboard.home',
  End: 'key.keyboard.end',
  PageUp: 'key.keyboard.page.up',
  PageDown: 'key.keyboard.page.down',
  Minus: 'key.keyboard.minus',
  Equal: 'key.keyboard.equal',
  BracketLeft: 'key.keyboard.left.bracket',
  BracketRight: 'key.keyboard.right.bracket',
  Backslash: 'key.keyboard.backslash',
  Semicolon: 'key.keyboard.semicolon',
  Quote: 'key.keyboard.apostrophe',
  Backquote: 'key.keyboard.grave',
  Comma: 'key.keyboard.comma',
  Period: 'key.keyboard.period',
  Slash: 'key.keyboard.slash',
}

const sectionCls = 'rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900'
const labelCls = 'text-sm text-neutral-600 dark:text-neutral-300'

/** 标量字段名 */
type ScalarField =
  | 'fov'
  | 'guiScale'
  | 'brightness'
  | 'renderDistance'
  | 'maxFps'
  | 'fullscreen'
  | 'mouseSensitivity'
  | 'autoJump'
  | 'masterVolume'

/** 各标量字段的游戏默认值（未设置时展示用） */
const DEFAULTS: Record<ScalarField, number | boolean> = {
  fov: 70,
  guiScale: 0,
  brightness: 0.5,
  renderDistance: 12,
  maxFps: 120,
  fullscreen: true,
  mouseSensitivity: 0.5,
  autoJump: false,
  masterVolume: 1,
}

export function GameConfigView() {
  const [instances, setInstances] = useState<InstanceSummary[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)
  const [applying, setApplying] = useState(false)
  const [syncOpen, setSyncOpen] = useState(false)
  const [syncTargets, setSyncTargets] = useState<Set<string>>(new Set())
  const [syncResult, setSyncResult] = useState<string | null>(null)

  // 表单状态：全部以具体值呈现（未设置项用游戏默认），customized 记录已设置的项
  const [values, setValues] = useState<Record<ScalarField, number | boolean>>({
    ...DEFAULTS,
  })
  const [customized, setCustomized] = useState<Set<ScalarField>>(new Set())
  const [keys, setKeys] = useState<Record<string, string>>({})
  // 正在等待按键绑定的键位 action（null = 未在绑定中）
  const [binding, setBinding] = useState<string | null>(null)

  // 绑定监听：捕获阶段拦截所有按键（Tab / 空格 / F 键等不触发浏览器默认行为）；
  // Escape = 取消本次绑定等待，其余可识别按键直接写入绑定
  useEffect(() => {
    if (!binding) return
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      setBinding(null)
      if (e.code === 'Escape') return
      const symbol = CODE_SYMBOLS[e.code]
      if (!symbol) return // 无法识别的按键：忽略
      setKeys((k) => ({ ...k, [binding]: symbol }))
      setSaved(false)
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [binding])

  const load = useCallback(async () => {
    try {
      const t = await ipc.loadGameConfig()
      setCustomized(new Set())
      setKeys(t.keys ?? {})
      // 注意用 != null（undefined 与 null 都视为未设置）：
      // 旧版本 Rust 序列化 Option::None 会输出 null，宽松判断避免把 null 当值用
      const next = { ...DEFAULTS }
      if (t.fov != null) next.fov = t.fov
      if (t.guiScale != null) next.guiScale = t.guiScale
      if (t.brightness != null) next.brightness = t.brightness
      if (t.renderDistance != null) next.renderDistance = t.renderDistance
      if (t.maxFps != null) next.maxFps = t.maxFps
      if (t.fullscreen != null) next.fullscreen = t.fullscreen
      if (t.mouseSensitivity != null) next.mouseSensitivity = t.mouseSensitivity
      if (t.autoJump != null) next.autoJump = t.autoJump
      if (t.masterVolume != null) next.masterVolume = t.masterVolume
      setValues(next)
      setCustomized(
        new Set(
          (Object.keys(DEFAULTS) as ScalarField[]).filter(
            (k) => t[k] != null,
          ),
        ),
      )
      setSaved(false)
      setError(null)
    } catch (e) {
      setError(String(e))
    }
  }, [])

  const refreshInstances = useCallback(async () => {
    try {
      setInstances(await ipc.listInstances())
    } catch {
      /* 列表加载失败：保持现状 */
    }
  }, [])

  useEffect(() => {
    void load()
    void refreshInstances()
    let unlisten: (() => void) | undefined
    onInstancesChanged(() => refreshInstances()).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [load, refreshInstances])

  /** 由当前编辑状态构造模板：仅含已设置的项 */
  const buildTemplate = useCallback((): GameConfigTemplate => {
    const t: GameConfigTemplate = { keys }
    if (customized.has('fov')) t.fov = values.fov as number
    if (customized.has('guiScale')) t.guiScale = values.guiScale as number
    if (customized.has('brightness')) t.brightness = values.brightness as number
    if (customized.has('renderDistance')) t.renderDistance = values.renderDistance as number
    if (customized.has('maxFps')) t.maxFps = values.maxFps as number
    if (customized.has('fullscreen')) t.fullscreen = values.fullscreen as boolean
    if (customized.has('mouseSensitivity')) t.mouseSensitivity = values.mouseSensitivity as number
    if (customized.has('autoJump')) t.autoJump = values.autoJump as boolean
    if (customized.has('masterVolume')) t.masterVolume = values.masterVolume as number
    return t
  }, [customized, keys, values])

  /** 已设置的项总数（含键位差异） */
  const setCount = useMemo(() => {
    let n = customized.size
    n += Object.keys(keys).filter((a) => {
      const def = KEY_DEFS.find((d) => d.action === a)?.def
      return def !== keys[a]
    }).length
    return n
  }, [customized, keys])

  const setField = (key: ScalarField, value: number | boolean) => {
    setValues((v) => ({ ...v, [key]: value }))
    setCustomized((s) => new Set(s).add(key))
    setSaved(false)
  }

  /** 重置单项为「未设置」（应用时跳过该项） */
  const resetField = (key: ScalarField) => {
    setValues((v) => ({ ...v, [key]: DEFAULTS[key] }))
    setCustomized((s) => {
      const next = new Set(s)
      next.delete(key)
      return next
    })
    setSaved(false)
  }

  const resetKey = (action: string) => {
    setKeys((k) => {
      const next = { ...k }
      delete next[action]
      return next
    })
    setSaved(false)
  }

  const handleSave = async () => {
    setApplying(true)
    setError(null)
    try {
      await ipc.saveGameConfig(buildTemplate())
      setSaved(true)
    } catch (e) {
      setError(String(e))
    } finally {
      setApplying(false)
    }
  }

  const handleApply = async () => {
    const t = buildTemplate()
    setApplying(true)
    setSyncResult(null)
    setError(null)
    const ok: string[] = []
    const failed: string[] = []
    for (const id of syncTargets) {
      const name = instances?.find((i) => i.id === id)?.name ?? id
      try {
        await ipc.writeGameConfig(id, t)
        ok.push(name)
      } catch {
        failed.push(name)
      }
    }
    setSyncResult(
      `已应用到 ${ok.length} 个实例${failed.length > 0 ? `；失败：${failed.join('、')}` : ''}`,
    )
    setSyncOpen(false)
    setApplying(false)
  }

  const upd = <K extends ScalarField>(key: K, value: (typeof DEFAULTS)[K]) => {
    setField(key, value)
  }

  /** 标量项头部：标题 + 当前值（醒目展示）+「默认」状态与重置按钮 */
  const FieldHeader = ({
    field,
    title,
    value,
  }: {
    field: ScalarField
    title: string
    value?: string
  }) => (
    <p className={`flex items-center gap-1.5 ${labelCls}`}>
      <span>
        {title}
        {value && <span className="ml-1 font-medium text-accent">{value}</span>}
      </span>
      {customized.has(field) ? (
        <button
          type="button"
          title="恢复为未设置（应用时跳过此项）"
          onClick={() => resetField(field)}
          className="text-xs text-neutral-400 underline-offset-2 transition-colors duration-150 hover:text-accent hover:underline"
        >
          重置
        </button>
      ) : (
        <span className="rounded-md bg-neutral-100 px-1.5 py-0.5 text-xs text-neutral-400 dark:bg-neutral-800">
          默认
        </span>
      )}
    </p>
  )

  return (
    <div className="flex h-full flex-col">
      {/* 头部：标题 + 保存 / 应用 */}
      <div className="flex flex-wrap items-center gap-3 px-6 pt-6">
        <h1 className="text-lg font-semibold">游戏配置</h1>
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={handleSave}
            disabled={applying}
            title="保存为启动器全局配置"
            className="rounded-lg border border-neutral-200 px-3 py-2 text-sm transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:hover:bg-neutral-800"
          >
            保存
          </button>
          {saved && <span className="text-sm text-emerald-500">已保存</span>}
          <button
            type="button"
            onClick={() => {
              setSyncTargets(new Set())
              setSyncOpen(true)
            }}
            disabled={applying || setCount === 0 || (instances?.length ?? 0) === 0}
            title="把已设置的项写入所选实例"
            className="rounded-lg bg-accent px-3.5 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            应用到实例…
          </button>
        </div>
      </div>
      <p className="px-6 pt-2 text-xs text-neutral-400 dark:text-neutral-500">
        在启动器里直接调整视场角、灵敏度、键位等设置并保存为全局配置；「应用到实例」只写入已设置的项
        （标记「默认」的项跳过），老版本键名 / 键码自动转换，其余 options.txt 内容不受影响
        {setCount > 0 && <span className="ml-1 text-accent">（当前已设置 {setCount} 项）</span>}
      </p>

      <main className="min-h-0 flex-1 overflow-y-auto p-6 pt-4">
        <div className="space-y-4">
          {error && <p className="text-sm text-red-500">{error}</p>}

          {/* 画面 */}
          <section className={sectionCls}>
            <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold">
              <IconConfig className="size-4" />
              画面
            </h2>
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <div>
                <FieldHeader field="fov" title="视场角（FOV）" value={`${values.fov}°`} />
                <input
                  type="range"
                  min={30}
                  max={110}
                  step={1}
                  value={values.fov as number}
                  onChange={(e) => upd('fov', Number(e.target.value))}
                  className="w-full accent-[var(--accent)]"
                />
                <p className="text-xs text-neutral-400">70° 为标准视角，110° 为 Quake Pro 超广角</p>
              </div>
              <div>
                <FieldHeader field="guiScale" title="界面缩放" />
                <select
                  className="w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
                  value={values.guiScale as number}
                  onChange={(e) => upd('guiScale', Number(e.target.value))}
                >
                  <option value={0}>自动</option>
                  <option value={1}>1（最小）</option>
                  <option value={2}>2</option>
                  <option value={3}>3</option>
                  <option value={4}>4（最大）</option>
                </select>
              </div>
              <div>
                <FieldHeader
                  field="brightness"
                  title="亮度"
                  value={`${Math.round((values.brightness as number) * 100)}%`}
                />
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.01}
                  value={values.brightness as number}
                  onChange={(e) => upd('brightness', Number(e.target.value))}
                  className="w-full accent-[var(--accent)]"
                />
                <p className="text-xs text-neutral-400">最左为昏暗，最右为明亮</p>
              </div>
              <div>
                <FieldHeader field="renderDistance" title="渲染距离" />
                <select
                  className="w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
                  value={values.renderDistance as number}
                  onChange={(e) => upd('renderDistance', Number(e.target.value))}
                >
                  {Array.from({ length: 30 }, (_, i) => i + 2).map((n) => (
                    <option key={n} value={n}>
                      {n} 区块
                    </option>
                  ))}
                </select>
              </div>
              <div>
                <FieldHeader field="maxFps" title="最大帧率" />
                <select
                  className="w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700"
                  value={values.maxFps as number}
                  onChange={(e) => upd('maxFps', Number(e.target.value))}
                >
                  <option value={260}>不限</option>
                  {[30, 60, 90, 120, 144, 165, 240].map((n) => (
                    <option key={n} value={n}>
                      {n} FPS
                    </option>
                  ))}
                </select>
              </div>
              <div className="flex items-end pb-1">
                <label className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={values.fullscreen as boolean}
                    onChange={(e) => upd('fullscreen', e.target.checked)}
                    className="size-4 accent-[var(--accent)]"
                  />
                  全屏启动
                </label>
              </div>
            </div>
          </section>

          {/* 控制与音频 */}
          <section className={sectionCls}>
            <h2 className="mb-3 text-sm font-semibold">控制与音频</h2>
            <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
              <div>
                <FieldHeader
                  field="mouseSensitivity"
                  title="鼠标灵敏度"
                  value={`${Math.round((values.mouseSensitivity as number) * 200)}%`}
                />
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.01}
                  value={values.mouseSensitivity as number}
                  onChange={(e) => upd('mouseSensitivity', Number(e.target.value))}
                  className="w-full accent-[var(--accent)]"
                />
                <p className="text-xs text-neutral-400">100% 为游戏默认灵敏度</p>
              </div>
              <div>
                <FieldHeader
                  field="masterVolume"
                  title="主音量"
                  value={`${Math.round((values.masterVolume as number) * 100)}%`}
                />
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.01}
                  value={values.masterVolume as number}
                  onChange={(e) => upd('masterVolume', Number(e.target.value))}
                  className="w-full accent-[var(--accent)]"
                />
              </div>
              <div className="flex items-center gap-2">
                <label className="flex cursor-pointer items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={values.autoJump as boolean}
                    onChange={(e) => upd('autoJump', e.target.checked)}
                    className="size-4 accent-[var(--accent)]"
                  />
                  自动跳跃
                </label>
                {customized.has('autoJump') ? (
                  <button
                    type="button"
                    onClick={() => resetField('autoJump')}
                    className="text-xs text-neutral-400 transition-colors duration-150 hover:text-accent"
                  >
                    重置
                  </button>
                ) : (
                  <span className="rounded-md bg-neutral-100 px-1.5 py-0.5 text-xs text-neutral-400 dark:bg-neutral-800">
                    默认
                  </span>
                )}
                <span className="text-xs text-neutral-400">（1.10+ 生效）</span>
              </div>
            </div>
          </section>

          {/* 快捷键 */}
          <section className={sectionCls}>
            <h2 className="mb-1 text-sm font-semibold">快捷键</h2>
            <p className="mb-3 text-xs text-neutral-400 dark:text-neutral-500">
              点击按键框后按下键盘任意键完成绑定（Esc 取消等待）；仅收录常用键位，1.13 前后键名 / 键码自动转换，未收录的自定义键位保持原样
            </p>
            <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
              {KEY_DEFS.map((def) => {
                const current = keys[def.action] ?? def.def
                const changed = keys[def.action] !== undefined
                return (
                  <div key={def.action} className="flex items-center gap-2">
                    <span className={`w-24 shrink-0 ${labelCls}`}>{def.label}</span>
                    <button
                      type="button"
                      onClick={() => setBinding(binding === def.action ? null : def.action)}
                      className={`h-9 w-28 shrink-0 rounded-lg border text-sm transition-colors duration-150 ${
                        binding === def.action
                          ? 'animate-pulse border-accent bg-accent-soft font-medium text-accent'
                          : 'border-neutral-200 bg-neutral-50 text-neutral-600 hover:border-accent dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-300'
                      }`}
                    >
                      {binding === def.action ? '按下按键…' : symbolLabel(current)}
                    </button>
                    {changed && (
                      <button
                        type="button"
                        title="取消绑定（恢复为不设置）"
                        onClick={() => {
                          if (binding === def.action) setBinding(null)
                          resetKey(def.action)
                        }}
                        className="flex size-6 shrink-0 items-center justify-center rounded-md text-neutral-400 transition-colors duration-150 hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-500/10"
                      >
                        <IconClose className="size-3.5" />
                      </button>
                    )}
                  </div>
                )
              })}
            </div>
          </section>
        </div>
      </main>

      {/* 应用目标选择弹窗 */}
      {syncOpen && (
        <div
          className="absolute inset-0 z-40 flex animate-[fade-in_120ms_ease-out] items-center justify-center bg-black/40"
          onClick={() => setSyncOpen(false)}
        >
          <div
            className="w-[26rem] animate-[modal-in_150ms_ease-out] rounded-xl border border-neutral-200 bg-white p-5 shadow-xl dark:border-neutral-800 dark:bg-neutral-900"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-sm font-semibold">应用到实例</h3>
            <p className="mt-1 text-xs text-neutral-400 dark:text-neutral-500">
              将已设置的 {setCount} 项写入勾选的实例（未设置的项跳过）
            </p>
            <div className="mt-3 max-h-60 space-y-1 overflow-y-auto">
              {instances === null && (
                <p className="py-4 text-center text-sm text-neutral-400">加载中…</p>
              )}
              {instances?.length === 0 && (
                <p className="py-4 text-center text-sm text-neutral-400">
                  暂无实例，请先在「实例」页创建
                </p>
              )}
              {instances?.map((i) => (
                <label
                  key={i.id}
                  className="flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                >
                  <input
                    type="checkbox"
                    checked={syncTargets.has(i.id)}
                    onChange={(e) => {
                      const next = new Set(syncTargets)
                      if (e.target.checked) next.add(i.id)
                      else next.delete(i.id)
                      setSyncTargets(next)
                    }}
                    className="size-4 accent-[var(--accent)]"
                  />
                  <span className="truncate">{i.name}</span>
                  <span className="ml-auto text-xs text-neutral-400">{i.mcVersion}</span>
                </label>
              ))}
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setSyncOpen(false)}
                className="rounded-lg border border-neutral-200 px-3 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                取消
              </button>
              <button
                type="button"
                onClick={handleApply}
                disabled={applying || syncTargets.size === 0}
                className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
              >
                {applying ? '应用中…' : `应用（${syncTargets.size}）`}
              </button>
            </div>
          </div>
        </div>
      )}
      {syncResult && (
        <p className="px-6 pb-4 text-sm text-emerald-500">{syncResult}</p>
      )}
    </div>
  )
}
