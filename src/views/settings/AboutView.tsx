// 「关于」子页（设置页底部入口卡片进入）：关于 / 依赖 / 法律声明 三块，仿 HMCL 布局
// 依赖条目为静态清单（随 package.json / Cargo.toml 主要依赖人工维护），点击经 open_external 唤起系统浏览器

import { useEffect, useRef, useState } from 'react'
import { getVersion } from '@tauri-apps/api/app'
import { ipc } from '../../lib/ipc'
import { IconExternal } from '../../components/common/Icons'
import { Section } from './SettingsView'

/** 依赖条目（名称 + 版权/署名 + 许可证 + 仓库地址） */
interface DepEntry {
  name: string
  copyright: string
  license: string
  url: string
}

const DEPS_FRONTEND: DepEntry[] = [
  { name: 'React 19', copyright: 'Copyright © Meta Platforms, Inc. and affiliates.', license: 'MIT License', url: 'https://github.com/facebook/react' },
  { name: 'Zustand', copyright: 'Copyright © 2019 Paul Armstrong.', license: 'MIT License', url: 'https://github.com/pmndrs/zustand' },
  { name: 'skinview3d', copyright: 'Copyright © 2017-2024 Yezhi Chen (bs-community).', license: 'MIT License', url: 'https://github.com/bs-community/skinview3d' },
  { name: '@tauri-apps/api', copyright: 'Copyright © Tauri Programme within The Commons Conservancy.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/tauri-apps/tauri' },
  { name: 'Tailwind CSS', copyright: 'Copyright © Tailwind Labs, Inc.', license: 'MIT License', url: 'https://github.com/tailwindlabs/tailwindcss' },
  { name: 'TypeScript', copyright: 'Copyright © Microsoft Corporation.', license: 'Apache-2.0 License', url: 'https://github.com/microsoft/TypeScript' },
  { name: 'Vite', copyright: 'Copyright © 2019-present VoidZero Inc. and Vite contributors.', license: 'MIT License', url: 'https://github.com/vitejs/vite' },
]

const DEPS_BACKEND: DepEntry[] = [
  { name: 'Tauri', copyright: 'Copyright © Tauri Programme within The Commons Conservancy.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/tauri-apps/tauri' },
  { name: 'Tokio', copyright: 'Copyright © Tokio Contributors.', license: 'MIT License', url: 'https://github.com/tokio-rs/tokio' },
  { name: 'Reqwest', copyright: 'Copyright © Sean McArthur.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/seanmonstar/reqwest' },
  { name: 'Serde', copyright: 'Copyright © David Tolnay.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/serde-rs/serde' },
  { name: 'Axum', copyright: 'Copyright © Token Source LLC and David Pedersen.', license: 'MIT License', url: 'https://github.com/tokio-rs/axum' },
  { name: 'fastnbt', copyright: 'Copyright © owengage.', license: 'MIT License', url: 'https://github.com/owengage/fastnbt' },
  { name: 'hickory-resolver', copyright: 'Copyright © The Hickory Contributors.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/hickory-dns/hickory-dns' },
  { name: 'image', copyright: 'Copyright © The image-rs Developers.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/image-rs/image' },
  { name: 'sysinfo', copyright: 'Copyright © Guillaume Gomez.', license: 'MIT License', url: 'https://github.com/GuillaumeGomez/sysinfo' },
  { name: 'zip', copyright: 'Copyright © Zip-rs contributors.', license: 'MIT License', url: 'https://github.com/zip-rs/zip2' },
  { name: 'arboard', copyright: 'Copyright © Artur Kovacs and Avi Weinstock.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/1Password/arboard' },
  { name: 'trash', copyright: 'Copyright © Artur Kovacs.', license: 'MIT License', url: 'https://github.com/Byron/trash-rs' },
  { name: 'opener', copyright: 'Copyright © 2019 Brian Bowman.', license: 'MIT License', url: 'https://github.com/Seeker1437/opener' },
  { name: 'RustCrypto SHA-1 / SHA-2', copyright: 'Copyright © RustCrypto Developers.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/RustCrypto/hashes' },
  { name: 'toml', copyright: 'Copyright © David Tolnay and Toml-rs contributors.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/toml-rs/toml' },
  { name: 'base64', copyright: 'Copyright © 2015 Alice Ryhl and contributors.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/marshallpierce/rust-base64' },
  { name: 'winreg', copyright: 'Copyright © Igor Shaula.', license: 'MIT License', url: 'https://github.com/gentoo90/winreg-rs' },
  { name: 'windows-sys', copyright: 'Copyright © Microsoft Corporation.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/microsoft/windows-rs' },
  { name: 'thiserror', copyright: 'Copyright © David Tolnay.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/dtolnay/thiserror' },
  { name: 'uuid', copyright: 'Copyright © The uuid-rs Contributors.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/uuid-rs/uuid' },
  { name: 'hex', copyright: 'Copyright © 2013-2016 KokaKiwi and rust-hex contributors.', license: 'MIT / Apache-2.0 License', url: 'https://github.com/KokaKiwi/rust-hex' },
]

const DEPS_RUNTIME: DepEntry[] = [
  { name: 'frpc（樱花内网穿透客户端）', copyright: 'Copyright © SakuraFrp (natfrp.com)，按其服务条款使用。', license: '专有软件', url: 'https://www.natfrp.com/' },
  { name: 'authlib-injector', copyright: 'Copyright © yushijinhun.', license: 'MIT License', url: 'https://github.com/yushijinhun/authlib-injector' },
]

/** 打开外部链接（失败在行内提示） */
function useOpenExternal() {
  const [error, setError] = useState<string | null>(null)
  const open = (url: string) => {
    ipc
      .openExternal(url)
      .then(() => setError(null))
      .catch((e) => setError(String(e).replace(/^Error:\s*/i, '')))
  }
  return { open, error }
}

function DepGroup({
  title,
  deps,
  onOpen,
}: {
  title: string
  deps: DepEntry[]
  onOpen: (url: string) => void
}) {
  return (
    <div className="mt-1">
      <p className="mb-1 text-xs font-medium text-neutral-400 dark:text-neutral-500">{title}</p>
      {deps.map((d) => (
        <button
          key={d.name}
          type="button"
          onClick={() => onOpen(d.url)}
          className="flex w-full items-start justify-between gap-3 border-b border-neutral-100 py-2.5 text-left last:border-b-0 transition-colors duration-150 hover:bg-neutral-50 dark:border-neutral-800/60 dark:hover:bg-neutral-800/40"
        >
          <span className="min-w-0">
            <span className="block text-sm font-medium">{d.name}</span>
            <span className="mt-0.5 block text-xs text-neutral-500 dark:text-neutral-400">{d.copyright}</span>
            <span className="mt-0.5 block text-xs text-neutral-400 dark:text-neutral-500">Licensed under the {d.license}.</span>
          </span>
          <IconExternal className="mt-1 size-4 shrink-0 text-neutral-400 dark:text-neutral-500" />
        </button>
      ))}
    </div>
  )
}

export function AboutView({ onBack }: { onBack: () => void }) {
  const [version, setVersion] = useState('')
  const { open, error } = useOpenExternal()
  const rootRef = useRef<HTMLDivElement>(null)

  // 进入子页时把滚动容器（最近的 overflow-y-auto 祖先，即 AppShell 的 <main>）
  // 回到顶部——切换子视图时滚动条位置会被保留，页面会停在中间
  useEffect(() => {
    let p = rootRef.current?.parentElement
    while (p) {
      const ov = getComputedStyle(p).overflowY
      if (ov === 'auto' || ov === 'scroll') {
        p.scrollTop = 0
        break
      }
      p = p.parentElement
    }
  }, [])

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(''))
  }, [])

  const backBtn =
    'rounded-lg px-2.5 py-1.5 text-sm text-neutral-500 transition-colors duration-150 hover:bg-neutral-100 dark:text-neutral-400 dark:hover:bg-neutral-800'

  return (
    <div ref={rootRef} className="mx-auto max-w-2xl space-y-4 p-6">
      <div className="flex items-center gap-3">
        <button type="button" onClick={onBack} className={backBtn}>
          ← 返回设置
        </button>
        <h1 className="text-lg font-semibold">关于</h1>
      </div>

      <Section title="关于">
        <p className="text-sm font-semibold">PieMCL{version && ` v${version}`}</p>
        <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
          一款使用 Tauri 构建的 Minecraft Java 版启动器
        </p>
        <p className="mt-1 text-xs text-neutral-500 dark:text-neutral-400">
          版权所有 © 2026 R3_141 及贡献者
        </p>
        <div className="mt-3 space-y-0.5">
          {[
            { label: '启动器官网', desc: 'PieMCL.top', url: 'https://piemcl.top' },
            { label: 'GitHub 开源地址', desc: 'https://github.com/R3-141/PieMCL', url: 'https://github.com/R3-141/PieMCL' },
            { label: '作者 B 站主页', desc: 'https://space.bilibili.com/514312552', url: 'https://space.bilibili.com/514312552' },
            { label: '联系邮箱', desc: 'Meonby@outlook.com', url: 'mailto:Meonby@outlook.com' },
          ].map((row) => (
            <button
              key={row.label}
              type="button"
              onClick={() => open(row.url)}
              className="flex w-full items-center justify-between gap-3 rounded-lg px-2 py-2 text-left transition-colors duration-150 hover:bg-neutral-50 dark:hover:bg-neutral-800/60"
            >
              <span>
                <span className="block text-sm">{row.label}</span>
                <span className="mt-0.5 block truncate text-xs text-neutral-500 dark:text-neutral-400">{row.desc}</span>
              </span>
              <IconExternal className="size-4 shrink-0 text-neutral-400 dark:text-neutral-500" />
            </button>
          ))}
        </div>
      </Section>

      <Section title="依赖">
        <p className="text-xs text-neutral-500 dark:text-neutral-400">
          点击条目可跳转至对应项目仓库；各依赖的许可证以其仓库声明为准
        </p>
        <DepGroup title="前端" deps={DEPS_FRONTEND} onOpen={open} />
        <DepGroup title="Rust 后端" deps={DEPS_BACKEND} onOpen={open} />
        <DepGroup title="运行时组件" deps={DEPS_RUNTIME} onOpen={open} />
      </Section>

      <Section title="法律声明">
        <div className="space-y-3">
          <div>
            <p className="text-sm font-medium">版权</p>
            <p className="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
              版权所有 © 2026 R3_141 及贡献者
            </p>
          </div>
          <button
            type="button"
            onClick={() => open('https://www.gnu.org/licenses/gpl-3.0.html')}
            className="flex w-full items-center justify-between gap-3 rounded-lg px-2 py-2 text-left transition-colors duration-150 hover:bg-neutral-50 dark:hover:bg-neutral-800/60"
          >
            <span>
              <span className="block text-sm font-medium">开源</span>
              <span className="mt-0.5 block text-xs text-neutral-500 dark:text-neutral-400">
                GPL v3（https://github.com/R3-141/PieMCL），点击查看许可证全文
              </span>
            </span>
            <IconExternal className="size-4 shrink-0 text-neutral-400 dark:text-neutral-500" />
          </button>
          <div>
            <p className="text-sm font-medium">Minecraft 相关声明</p>
            <p className="mt-0.5 text-xs leading-5 text-neutral-500 dark:text-neutral-400">
              本启动器与 Mojang Studios / Microsoft 无从属关系，亦未获官方认可。
              Minecraft 为 Mojang Studios 的商标；使用本启动器即表示您已同意 Minecraft 最终用户许可协议（EULA）。
            </p>
          </div>
          <div>
            <p className="text-sm font-medium">联机服务声明</p>
            <p className="mt-0.5 text-xs leading-5 text-neutral-500 dark:text-neutral-400">
              本启动器仅提供接口，不提供穿透服务，相关规则由 SakuraFrp 决定。
            </p>
          </div>
        </div>
      </Section>

      {error && <p className="text-xs text-red-500">{error}</p>}
    </div>
  )
}
