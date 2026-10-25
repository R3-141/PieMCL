import { useEffect, useRef, useState } from 'react'
import { NAV_MAIN } from '../../lib/nav'
import { useUiStore } from '../../stores/uiStore'
import { useBackgroundStore } from '../../stores/backgroundStore'
import { effectiveIsDark } from '../../lib/theme'
import { ipc } from '../../lib/ipc'
import type { AppSettings } from '../../types/domain'
import type { ThemeMode } from '../../types/ui'
import { BackgroundCropDialog } from './BackgroundCropDialog'
import { AboutView } from './AboutView'

const THEME_OPTIONS: readonly { value: ThemeMode; label: string }[] = [
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
  { value: 'system', label: '跟随系统' },
]

const inputCls =
  'w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'
const labelCls = 'mb-1.5 block text-sm font-medium'

/** 轻量开关（本文件内复用，暂不抽公共组件） */
function Switch({
  checked,
  onChange,
  label,
  desc,
}: {
  checked: boolean
  onChange: (v: boolean) => void
  label: string
  desc?: string
}) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-4 py-2">
      <span>
        <span className="block text-sm">{label}</span>
        {desc && <span className="mt-0.5 block text-xs text-neutral-500 dark:text-neutral-400">{desc}</span>}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative h-6 w-11 shrink-0 rounded-full transition-colors duration-200 ${
          checked ? 'bg-accent' : 'bg-neutral-300 dark:bg-neutral-700'
        }`}
      >
        <span
          className={`absolute top-0.5 size-5 rounded-full bg-white shadow transition-[left] duration-200 ${
            checked ? 'left-[22px]' : 'left-0.5'
          }`}
        />
      </button>
    </label>
  )
}

export function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900">
      <h2 className="mb-3 text-sm font-semibold">{title}</h2>
      {children}
    </section>
  )
}

/** 下载源与并发设置（镜像列表实时生效；并发数修改需重启） */
function DownloadSourceSection() {
  const [mirrorsText, setMirrorsText] = useState<string | null>(null)
  const [downloadSource, setDownloadSource] = useState('auto')
  const [globalConcurrency, setGlobalConcurrency] = useState(64)
  const [perHostConcurrency, setPerHostConcurrency] = useState(16)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ipc
      .getSettings()
      .then((s: AppSettings) => {
        setMirrorsText(s.mirrors.join('\n'))
        setDownloadSource(s.downloadSource || 'auto')
        setGlobalConcurrency(s.globalConcurrency)
        setPerHostConcurrency(s.perHostConcurrency)
      })
      .catch((e) => setError(String(e)))
  }, [])

  const handleSave = async () => {
    if (mirrorsText === null) return
    setSaving(true)
    setError(null)
    setMessage(null)
    try {
      // 合并现值保存（defaultPlayerName 等其他字段保持不变）
      const cur = await ipc.getSettings()
      const s = await ipc.saveSettings({
        ...cur,
        mirrors: mirrorsText
          .split('\n')
          .map((l) => l.trim())
          .filter((l) => l.length > 0),
        downloadSource,
        globalConcurrency,
        perHostConcurrency,
      })
      setMirrorsText(s.mirrors.join('\n'))
      setDownloadSource(s.downloadSource || 'auto')
      setGlobalConcurrency(s.globalConcurrency)
      setPerHostConcurrency(s.perHostConcurrency)
      setMessage('已保存：下载源与镜像列表实时生效；并发数修改需重启应用生效')
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  const dirty = mirrorsText !== null

  return (
    <Section title="下载源与并发">
      <div className="space-y-3">
        <div>
          <label className={labelCls} htmlFor="set-dl-source">下载源策略</label>
          <select
            id="set-dl-source"
            className={inputCls}
            value={downloadSource}
            onChange={(e) => setDownloadSource(e.target.value)}
            disabled={saving || !dirty}
          >
            <option value="auto">自动（官方直连优先，失败轮换镜像）</option>
            <option value="official">仅官方源（不走镜像）</option>
            <option value="mirror">镜像优先（国内网络推荐）</option>
          </select>
          <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
            官方直连速度通常更快；若你的网络访问 Mojang 官方域名受限，可选镜像优先。
          </p>
        </div>
        <div>
          <label className={labelCls} htmlFor="set-mirrors">镜像源列表</label>
          <textarea
            id="set-mirrors"
            className={`${inputCls} h-24 resize-none font-mono text-xs`}
            placeholder={'每行一个镜像源，例如：\nhttps://bmclapi2.bangbang93.com'}
            value={mirrorsText ?? ''}
            onChange={(e) => setMirrorsText(e.target.value)}
            disabled={saving || !dirty}
          />
          <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
            仅改写 Mojang 官方域名（版本元数据、资源、库文件）；Fabric / Quilt 等第三方源不受影响。
            下载失败时自动轮换下一候选，官方源恒为兜底。
          </p>
        </div>
        <div className="flex gap-4">
          <div className="flex-1">
            <label className={labelCls} htmlFor="set-global">全局并发上限</label>
            <input
              id="set-global"
              type="number"
              min={1}
              max={256}
              className={inputCls}
              value={globalConcurrency}
              onChange={(e) => setGlobalConcurrency(Number(e.target.value))}
              disabled={saving || !dirty}
            />
          </div>
          <div className="flex-1">
            <label className={labelCls} htmlFor="set-host">单主机并发上限</label>
            <input
              id="set-host"
              type="number"
              min={1}
              max={64}
              className={inputCls}
              value={perHostConcurrency}
              onChange={(e) => setPerHostConcurrency(Number(e.target.value))}
              disabled={saving || !dirty}
            />
          </div>
        </div>
        {message && <p className="animate-[fade-in_150ms_ease-out] text-xs text-emerald-600 dark:text-emerald-400">{message}</p>}
        {error && <p className="text-xs text-red-500">{error}</p>}
        <button
          type="button"
          onClick={handleSave}
          disabled={saving || !dirty}
          className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
        >
          {saving ? '保存中…' : '保存下载设置'}
        </button>
      </div>
    </Section>
  )
}

/** 网络代理：直连 / 跟随系统 / 手动地址（Modrinth 等国外源直连不畅时可开代理） */
function ProxySection() {
  const [mode, setMode] = useState<string | null>(null)
  const [url, setUrl] = useState('')
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ipc
      .getSettings()
      .then((s: AppSettings) => {
        setMode(s.proxyMode || 'none')
        setUrl(s.proxyUrl || '')
      })
      .catch((e) => setError(String(e)))
  }, [])

  const handleSave = async () => {
    if (mode === null) return
    setSaving(true)
    setError(null)
    setMessage(null)
    try {
      const cur = await ipc.getSettings()
      await ipc.saveSettings({
        ...cur,
        proxyMode: mode,
        proxyUrl: url.trim(),
      })
      setMessage('已保存：模组市场等请求级连接立即生效，游戏资源下载需重启应用')
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  const dirty = mode !== null

  return (
    <Section title="网络代理">
      <div className="space-y-3">
        <p className="text-xs text-neutral-500 dark:text-neutral-400">
          直连无法访问 Modrinth / OptiFine 官网等国外源时，可在此配置代理（与浏览器/系统代理工具配合使用）
        </p>
        <div>
          <label className={labelCls} htmlFor="set-proxy-mode">代理模式</label>
          <select
            id="set-proxy-mode"
            className={inputCls}
            value={mode ?? 'none'}
            onChange={(e) => setMode(e.target.value)}
            disabled={saving || !dirty}
          >
            <option value="none">直连（不使用代理）</option>
            <option value="system">跟随系统代理（PAC 模式不受支持）</option>
            <option value="manual">手动指定代理（推荐，最可靠）</option>
          </select>
        </div>
        {mode === 'manual' && (
          <div>
            <label className={labelCls} htmlFor="set-proxy-url">代理地址</label>
            <input
              id="set-proxy-url"
              type="text"
              className={inputCls}
              placeholder="例如：http://127.0.0.1:7890 或 socks5://127.0.0.1:7890"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              disabled={saving}
            />
          </div>
        )}
        {message && <p className="animate-[fade-in_150ms_ease-out] text-xs text-emerald-600 dark:text-emerald-400">{message}</p>}
        {error && <p className="text-xs text-red-500">{error}</p>}
        <button
          type="button"
          onClick={handleSave}
          disabled={saving || !dirty}
          className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
        >
          {saving ? '保存中…' : '保存代理设置'}
        </button>
      </div>
    </Section>
  )
}

/** 模组管理设置：回收站保留期 + CurseForge API Key */
function ModsSection() {
  const [days, setDays] = useState<number | null>(null)
  const [cfKey, setCfKey] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ipc
      .getSettings()
      .then((s: AppSettings) => {
        setDays(s.trashRetentionDays)
        setCfKey(s.curseforgeApiKey)
      })
      .catch((e) => setError(String(e)))
  }, [])

  const handleSave = async () => {
    if (days === null || cfKey === null) return
    setSaving(true)
    setError(null)
    setMessage(null)
    try {
      const cur = await ipc.getSettings()
      await ipc.saveSettings({
        ...cur,
        trashRetentionDays: Math.max(0, Math.min(3650, days)),
        curseforgeApiKey: cfKey.trim(),
      })
      setMessage('已保存')
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Section title="模组管理">
      <div className="flex items-end gap-4">
        <div className="w-40">
          <label className={labelCls} htmlFor="set-trash-days">回收站保留天数</label>
          <input
            id="set-trash-days"
            type="number"
            min={0}
            max={3650}
            className={inputCls}
            value={days ?? ''}
            onChange={(e) => setDays(Number(e.target.value))}
            disabled={saving || days === null}
          />
        </div>
        <span className="pb-2.5 text-xs text-neutral-500 dark:text-neutral-400">
          删除的模组在实例回收站中保留这些天数后自动清除；0 = 删除即永久移除
        </span>
      </div>
      <div className="mt-3">
        <label className={labelCls} htmlFor="set-cf-key">CurseForge API Key（可选）</label>
        <input
          id="set-cf-key"
          type="password"
          className={inputCls}
          placeholder="留空仅使用 Modrinth；填入后可在模组市场切换 CurseForge 源"
          value={cfKey ?? ''}
          onChange={(e) => setCfKey(e.target.value)}
          disabled={saving || cfKey === null}
        />
        <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
          在 curseforge.com/settings/api 免费申请；仅存储在本机 settings.json
        </p>
      </div>
      {message && <p className="mt-2 animate-[fade-in_150ms_ease-out] text-xs text-emerald-600 dark:text-emerald-400">{message}</p>}
      {error && <p className="mt-2 text-xs text-red-500">{error}</p>}
      <div className="mt-3 flex items-center gap-3">
        <button
          type="button"
          onClick={handleSave}
          disabled={saving || days === null || cfKey === null}
          className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
        >
          {saving ? '保存中…' : '保存模组设置'}
        </button>
        <button
          type="button"
          onClick={async () => {
            setError(null)
            setMessage(null)
            try {
              const n = await ipc.clearMarketCache()
              setMessage(n > 0 ? `已清除市场缓存（${n} 个图标），重新进入市场页面即会重新拉取` : '市场缓存已是空的')
            } catch (e) {
              setError(String(e))
            }
          }}
          className="rounded-lg bg-neutral-100 px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
        >
          刷新市场缓存
        </button>
      </div>
    </Section>
  )
}

/** 数据目录：显示当前根目录路径并提供打开入口 */
function DataDirSection() {
  const [dir, setDir] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ipc
      .getDataDir()
      .then(setDir)
      .catch((e) => setError(String(e)))
  }, [])

  return (
    <Section title="数据目录">
      <p className="text-xs text-neutral-500 dark:text-neutral-400">
        实例、Java 运行时、皮肤、账户与设置均存放在该目录下；便携版随启动器目录整体迁移
      </p>
      <div className="mt-2 flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded-lg bg-neutral-100 px-3 py-2 font-mono text-xs dark:bg-neutral-800">
          {dir ?? '读取中…'}
        </code>
        <button
          type="button"
          onClick={() => ipc.openDataDir().catch((e) => setError(String(e)))}
          disabled={dir === null}
          className="shrink-0 rounded-lg bg-neutral-100 px-3 py-2 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
        >
          打开文件夹
        </button>
      </div>
      {error && <p className="mt-2 text-xs text-red-500">{error}</p>}
    </Section>
  )
}

/** data URL → ArrayBuffer（保存背景时回传原图） */
function dataUrlToBytes(url: string): ArrayBuffer {
  const b64 = url.slice(url.indexOf(',') + 1)
  const bin = atob(b64)
  const arr = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) arr[i] = bin.charCodeAt(i)
  return arr.buffer
}

/** 个性化背景：上传裁剪背景图 + 模糊 / 遮罩调节（铺满全窗口，启用时替代系统磨砂） */
function BackgroundSection() {
  const [thumb, setThumb] = useState<string | null>(null)
  const [blur, setBlur] = useState(0)
  const [dim, setDim] = useState(25)
  const [uiTheme, setUiTheme] = useState('auto')
  // 卡片样式（全局 UI 偏好，经 uiStore 持久化）
  const cardOpacity = useUiStore((s) => s.cardOpacity)
  const setCardOpacity = useUiStore((s) => s.setCardOpacity)
  const cardBlur = useUiStore((s) => s.cardBlur)
  const setCardBlur = useUiStore((s) => s.setCardBlur)
  const cardTint = useUiStore((s) => s.cardTint)
  const setCardTint = useUiStore((s) => s.setCardTint)
  // 生效明暗（仅用于颜色选择器的预览初值）
  const themeMode = useUiStore((s) => s.theme)
  const bgHas = useBackgroundStore((s) => s.dataUrl !== null)
  const bgTheme = useBackgroundStore((s) => s.theme)
  const isDark = effectiveIsDark(themeMode, bgHas, bgTheme)
  const [cropOpen, setCropOpen] = useState(false)
  const [cropSrc, setCropSrc] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const saveTimer = useRef<number | null>(null)

  useEffect(() => {
    ipc
      .getSettings()
      .then((s) => {
        setBlur(s.backgroundBlur)
        setDim(s.backgroundDim)
        setUiTheme(s.backgroundTheme || 'auto')
      })
      .catch((e) => setError(String(e)))
    ipc
      .readBackgroundImage()
      .then(setThumb)
      .catch(() => {})
  }, [])

  const handlePick = async () => {
    setError(null)
    try {
      const d = await ipc.pickBackgroundImage()
      if (!d) return
      setCropSrc(d)
      setCropOpen(true)
    } catch (e) {
      setError(String(e))
    }
  }

  const handleReCrop = async () => {
    setError(null)
    try {
      const d = await ipc.readBackgroundOriginal()
      if (!d) {
        setError('原图缺失，请重新选择图片')
        return
      }
      setCropSrc(d)
      setCropOpen(true)
    } catch (e) {
      setError(String(e))
    }
  }

  const handleCropConfirm = async (bytes: ArrayBuffer) => {
    if (!cropSrc) return
    const original = dataUrlToBytes(cropSrc)
    const s = await ipc.saveBackgroundImage(bytes, original, 'jpg')
    setBlur(s.backgroundBlur)
    setDim(s.backgroundDim)
    const t = await ipc.readBackgroundImage()
    setThumb(t)
    useBackgroundStore.getState().apply(t, s.backgroundBlur, s.backgroundDim)
    setCropOpen(false)
    setCropSrc(null)
  }

  /** 模糊 / 遮罩滑块：预览即时生效，持久化防抖 400ms */
  const handleParams = (b: number, d: number) => {
    setBlur(b)
    setDim(d)
    useBackgroundStore.getState().setParams(b, d)
    if (saveTimer.current) window.clearTimeout(saveTimer.current)
    saveTimer.current = window.setTimeout(async () => {
      try {
        const cur = await ipc.getSettings()
        await ipc.saveSettings({ ...cur, backgroundBlur: b, backgroundDim: d })
      } catch {
        /* 滑块保存失败不打断预览 */
      }
    }, 400)
  }

  /** 界面配色下拉：背景启用时强制按钮 / 图标 / 文字明暗（保存 + 同步全局根类） */
  const handleUiTheme = async (t: string) => {
    setUiTheme(t)
    useBackgroundStore.getState().setTheme(t)
    try {
      const cur = await ipc.getSettings()
      await ipc.saveSettings({ ...cur, backgroundTheme: t })
    } catch (e) {
      setError(String(e))
    }
  }

  const handleRemove = async () => {
    setError(null)
    try {
      const s = await ipc.removeBackgroundImage()
      setThumb(null)
      useBackgroundStore.getState().apply(null, s.backgroundBlur, s.backgroundDim)
    } catch (e) {
      setError(String(e))
    }
  }

  const btn =
    'rounded-lg bg-neutral-100 px-3 py-1.5 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700'

  return (
    <Section title="个性化背景">
      <p className="text-xs text-neutral-500 dark:text-neutral-400">
        背景图铺满整个启动器窗口，启用时自动关闭窗口磨砂效果。
      </p>
      {thumb ? (
        <>
          <div className="mt-3 h-40 overflow-hidden rounded-lg border border-neutral-200 dark:border-neutral-800">
            <img src={thumb} alt="背景预览" className="size-full object-cover" />
          </div>
          <div className="mt-3 flex gap-2">
            <button type="button" onClick={handlePick} className={btn}>
              更换图片
            </button>
            <button type="button" onClick={handleReCrop} className={btn}>
              重新裁剪
            </button>
            <button type="button" onClick={handleRemove} className={btn}>
              移除背景
            </button>
          </div>
          {/* 界面配色：背景启用时的界面深浅主控（优先于「主题」设置） */}
          <div className="mt-4">
            <label className={labelCls} htmlFor="set-bg-theme">界面配色</label>
            <select
              id="set-bg-theme"
              className={inputCls}
              value={uiTheme}
              onChange={(e) => handleUiTheme(e.target.value)}
            >
              <option value="auto">自动（跟随「主题」设置）</option>
              <option value="dark">深色（适合亮色背景）</option>
              <option value="light">浅色（适合深色背景）</option>
            </select>
            <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
              启用背景后，此处是界面深浅的主控：按钮、图标、文字与卡片明暗都跟随此项，
              「主题」设置不再生效（移除背景后恢复）。
            </p>
          </div>
          <div className="mt-4 flex items-center gap-3">
            <span className="w-16 shrink-0 text-xs text-neutral-500 dark:text-neutral-400">
              模糊程度
            </span>
            <input
              type="range"
              min={0}
              max={40}
              step={1}
              value={blur}
              onChange={(e) => handleParams(Number(e.target.value), dim)}
              className="flex-1 accent-[var(--accent)]"
            />
            <span className="w-10 shrink-0 text-right text-xs text-neutral-500 dark:text-neutral-400">
              {blur}px
            </span>
          </div>
          <div className="mt-2 flex items-center gap-3">
            <span className="w-16 shrink-0 text-xs text-neutral-500 dark:text-neutral-400">
              遮罩暗度
            </span>
            <input
              type="range"
              min={0}
              max={70}
              step={1}
              value={dim}
              onChange={(e) => handleParams(blur, Number(e.target.value))}
              className="flex-1 accent-[var(--accent)]"
            />
            <span className="w-10 shrink-0 text-right text-xs text-neutral-500 dark:text-neutral-400">
              {dim}%
            </span>
          </div>
          <div className="mt-4 flex items-center gap-3">
            <span className="w-16 shrink-0 text-xs text-neutral-500 dark:text-neutral-400">
              卡片不透明度
            </span>
            <input
              type="range"
              min={10}
              max={100}
              step={1}
              value={cardOpacity}
              onChange={(e) => {
                setCardOpacity(Number(e.target.value))
              }}
              className="flex-1 accent-[var(--accent)]"
            />
            <span className="w-10 shrink-0 text-right text-xs text-neutral-500 dark:text-neutral-400">
              {cardOpacity}%
            </span>
          </div>
          <Switch
            checked={cardBlur}
            onChange={setCardBlur}
            label="卡片磨砂"
            desc="卡片底色叠加背景模糊（Acrylic 质感），搭配较低的不透明度效果最佳"
          />
          <div className="mt-4">
            <label className={labelCls} htmlFor="set-card-tint">卡片颜色</label>
            <select
              id="set-card-tint"
              className={inputCls}
              value={cardTint ? 'custom' : 'auto'}
              onChange={(e) =>
                // 从跟随切到自定义时给一个初值（跟随当前生效主题的底色）
                setCardTint(
                  e.target.value === 'auto' ? null : (cardTint ?? (isDark ? '#171717' : '#ffffff')),
                )
              }
            >
              <option value="auto">跟随界面深浅（浅色白 / 深色黑）</option>
              <option value="custom">自定义颜色</option>
            </select>
            {cardTint && (
              <div className="mt-2 flex items-center gap-3">
                <input
                  type="color"
                  aria-label="自定义卡片颜色"
                  value={cardTint}
                  onChange={(e) => setCardTint(e.target.value)}
                  className="h-9 w-14 shrink-0 cursor-pointer rounded-lg border border-neutral-200 bg-transparent p-1 dark:border-neutral-700"
                />
                <span className="min-w-0 flex-1 text-xs text-neutral-500 dark:text-neutral-400">
                  浅色与深色主题下卡片均使用该颜色（配合透明度与磨砂）
                </span>
                <button type="button" onClick={() => setCardTint(null)} className={btn}>
                  恢复跟随主题
                </button>
              </div>
            )}
          </div>
        </>
      ) : (
        <button
          type="button"
          onClick={handlePick}
          className="mt-3 flex h-28 w-full flex-col items-center justify-center rounded-lg border-2 border-dashed border-neutral-300 text-neutral-500 transition-colors duration-150 hover:border-accent hover:text-accent dark:border-neutral-700 dark:text-neutral-400 dark:hover:border-accent dark:hover:text-accent"
        >
          <span className="text-sm font-medium">选择背景图片</span>
          <span className="mt-1 text-xs">支持 PNG / JPG / WEBP，确认前可裁剪取景</span>
        </button>
      )}
      {error && (
        <p className="mt-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-600 dark:bg-red-500/10 dark:text-red-300">
          {error}
        </p>
      )}
      <BackgroundCropDialog
        open={cropOpen}
        src={cropSrc}
        onCancel={() => {
          setCropOpen(false)
          setCropSrc(null)
        }}
        onConfirm={handleCropConfirm}
      />
    </Section>
  )
}

/** 设置：主题 / 主色 / 动画 / 窗口效果 / 入口显隐 / 数据目录 / 下载源 / 模组 / 关于 */
export function SettingsView() {
  const [showAbout, setShowAbout] = useState(false)
  const theme = useUiStore((s) => s.theme)
  const setTheme = useUiStore((s) => s.setTheme)
  const customAccent = useUiStore((s) => s.customAccent)
  const setCustomAccent = useUiStore((s) => s.setCustomAccent)
  const reducedMotion = useUiStore((s) => s.reducedMotion)
  const setReducedMotion = useUiStore((s) => s.setReducedMotion)
  const blurEnabled = useUiStore((s) => s.blurEnabled)
  const setBlurEnabled = useUiStore((s) => s.setBlurEnabled)
  const blurStrength = useUiStore((s) => s.blurStrength)
  const setBlurStrength = useUiStore((s) => s.setBlurStrength)
  const blurTint = useUiStore((s) => s.blurTint)
  const setBlurTint = useUiStore((s) => s.setBlurTint)
  const hiddenNavItems = useUiStore((s) => s.hiddenNavItems)
  const toggleNavItemHidden = useUiStore((s) => s.toggleNavItemHidden)

  // 「关于」独立子页（入口卡片进入，整页替换设置内容）
  if (showAbout) {
    return <AboutView onBack={() => setShowAbout(false)} />
  }

  return (
    <div className="mx-auto max-w-2xl space-y-4 p-6">
      <h1 className="text-lg font-semibold">设置</h1>

      <Section title="主题">
        <div className="flex gap-2">
          {THEME_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              type="button"
              onClick={() => setTheme(opt.value)}
              className={`rounded-lg px-4 py-2 text-sm transition-colors duration-150 ${
                theme === opt.value
                  ? 'bg-accent text-white'
                  : 'bg-neutral-100 text-neutral-600 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700'
              }`}
            >
              {opt.label}
            </button>
          ))}
        </div>
        <p className="mt-2 text-xs text-neutral-500 dark:text-neutral-400">
          全局界面深浅的默认设置；启用自定义背景后，以「个性化背景 → 界面配色」为准。
        </p>
      </Section>

      <Section title="主色">
        <div className="flex items-center gap-3">
          <input
            type="color"
            aria-label="自定义主色"
            value={customAccent ?? '#4f7cff'}
            onChange={(e) => setCustomAccent(e.target.value)}
            className="h-9 w-14 cursor-pointer rounded-lg border border-neutral-200 bg-transparent p-1 dark:border-neutral-700"
          />
          <span className="text-xs text-neutral-500 dark:text-neutral-400">
            选择自定义主色，按钮、选中态等高亮元素随之变化
          </span>
          <button
            type="button"
            onClick={() => setCustomAccent(null)}
            className="ml-auto rounded-lg bg-neutral-100 px-3 py-1.5 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
          >
            恢复默认
          </button>
        </div>
      </Section>

      <Section title="动画与性能">
        <Switch
          checked={reducedMotion}
          onChange={setReducedMotion}
          label="减少动画"
          desc="关闭界面过渡动画，适合偏好静态界面或性能受限的场景"
        />
      </Section>

      <Section title="窗口效果">
        <Switch
          checked={blurEnabled}
          onChange={setBlurEnabled}
          label="磨砂质感"
          desc="整个窗口透出系统 Acrylic 模糊背景；关闭后恢复实色外观"
        />
        {blurEnabled && (
          <div className="mt-2 space-y-4 border-t border-neutral-100 pt-3 dark:border-neutral-800">
            {/* 颜色透明度 */}
            <div>
              <label className={labelCls} htmlFor="set-blur-strength">
                颜色透明度：<span className="text-accent">{blurStrength}%</span>
              </label>
              <input
                id="set-blur-strength"
                type="range"
                min={0}
                max={100}
                value={blurStrength}
                onChange={(e) => setBlurStrength(Number(e.target.value))}
                className="w-full accent-[var(--accent)]"
              />
              <div className="flex justify-between text-xs text-neutral-400">
                <span>全透出</span>
                <span>近实色</span>
              </div>
            </div>

            {/* 色调 */}
            <div className="flex items-center gap-3">
              <span className="text-sm">框架色调</span>
              <input
                type="color"
                aria-label="框架色调"
                value={blurTint ?? '#ffffff'}
                onChange={(e) => setBlurTint(e.target.value)}
                className="h-9 w-14 cursor-pointer rounded-lg border border-neutral-200 bg-transparent p-1 dark:border-neutral-700"
              />
              <span className="text-xs text-neutral-500 dark:text-neutral-400">
                标题栏 / 侧栏 / 内容区 / 下载抽屉按此色调分层
              </span>
              <button
                type="button"
                onClick={() => setBlurTint(null)}
                disabled={blurTint === null}
                className="ml-auto rounded-lg bg-neutral-100 px-3 py-1.5 text-xs text-neutral-600 transition-colors duration-150 hover:bg-neutral-200 disabled:opacity-50 dark:bg-neutral-800 dark:text-neutral-300 dark:hover:bg-neutral-700"
              >
                跟随主题
              </button>
            </div>
          </div>
        )}
      </Section>

      <Section title="导航入口显隐">
        <p className="mb-2 text-xs text-neutral-500 dark:text-neutral-400">
          隐藏后入口将从左侧导航移除（设置入口始终可见，避免无法进入设置；「社区资源」分组不参与隐藏）
        </p>
        <div className="space-y-1">
          {/* 设置入口永不可隐藏：防止把自己关在门外；社区子项已迁入分组，不在此列出 */}
          {NAV_MAIN.filter((it) => it.id !== 'settings').map((it) => {
            const visible = !hiddenNavItems.includes(it.id)
            // 平级可隐藏项（不含设置）至少保留一个可见 → 隐藏满时禁用
            const lastVisible = visible && hiddenNavItems.length === NAV_MAIN.length - 2
            return (
              <label
                key={it.id}
                className={`flex items-center gap-2.5 rounded-lg px-2 py-2 text-sm transition-colors duration-150 ${
                  lastVisible ? 'opacity-50' : 'hover:bg-neutral-100 dark:hover:bg-neutral-800'
                }`}
              >
                <input
                  type="checkbox"
                  checked={visible}
                  disabled={lastVisible}
                  onChange={() => toggleNavItemHidden(it.id)}
                  className="size-4 accent-[var(--accent)]"
                />
                {it.label}
              </label>
            )
          })}
        </div>
      </Section>

      <DownloadSourceSection />
      <ProxySection />
      <ModsSection />
      <BackgroundSection />
      <DataDirSection />

      {/* 关于入口（点击进入独立子页） */}
      <button
        type="button"
        onClick={() => setShowAbout(true)}
        className="block w-full rounded-xl border border-neutral-200 bg-white p-5 text-left transition-colors duration-150 hover:border-accent dark:border-neutral-800 dark:bg-neutral-900"
      >
        <div className="flex items-center justify-between gap-3">
          <span>
            <span className="block text-sm font-semibold">关于 PieMCL</span>
            <span className="mt-1 block text-xs text-neutral-500 dark:text-neutral-400">
              版本信息 · 开源地址 · 依赖清单 · 法律声明
            </span>
          </span>
          <span className="text-xs text-neutral-400 dark:text-neutral-500">查看详情 →</span>
        </div>
      </button>
    </div>
  )
}
