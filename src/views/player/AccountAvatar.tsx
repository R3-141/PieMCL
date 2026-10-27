// 账户皮肤头像：从皮肤 PNG 裁剪头部区域（(8,8) 起 8x8 像素）渲染。
// 三种账户来源统一处理：微软 = 激活皮肤 URL（fetch 转 data URL）；
// 离线 / 皮肤站 = 本地 IPC 拿 PNG data URL。无皮肤或加载失败回落首字母。

import { useEffect, useRef, useState } from 'react'
import { ipc } from '../../lib/ipc'
import type { AccountView } from '../../types/domain'

/** 按账户类型抓取皮肤贴图 data URL */
async function loadSkinTexture(account: AccountView): Promise<string | null> {
  try {
    if (account.kind === 'microsoft') {
      const url = account.skins.find((s) => s.state === 'ACTIVE')?.url
      if (!url) return null
      const resp = await fetch(url)
      if (!resp.ok) return null
      const blob = await resp.blob()
      return await new Promise<string | null>((resolve) => {
        const reader = new FileReader()
        reader.onload = () => resolve(String(reader.result))
        reader.onerror = () => resolve(null)
        reader.readAsDataURL(blob)
      })
    }
    if (account.kind === 'offline') {
      return await ipc.getOfflineSkinDataUrl(account.id)
    }
    if (account.kind === 'authlib') {
      const d = await ipc.getAuthlibSkinData(account.id)
      return d?.dataUrl ?? null
    }
  } catch {
    /* 加载失败回落首字母头像 */
  }
  return null
}

export function AccountAvatar({ account, size = 32 }: { account: AccountView; size?: number }) {
  const [texture, setTexture] = useState<string | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)

  const activeSkinUrl = account.skins.find((s) => s.state === 'ACTIVE')?.url ?? null
  useEffect(() => {
    let alive = true
    setTexture(null)
    void loadSkinTexture(account).then((t) => {
      if (alive) setTexture(t)
    })
    return () => {
      alive = false
    }
    // 皮肤变化来源：离线 fetchedAt / 微软激活皮肤 URL / 账户本身
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account.id, account.kind, account.offlineSkin?.fetchedAt, activeSkinUrl])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || !texture) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    const img = new Image()
    img.onload = () => {
      ctx.imageSmoothingEnabled = false // 像素风最近邻放大
      ctx.clearRect(0, 0, size, size)
      // 头部底层：(8,8) 起 8x8
      ctx.drawImage(img, 8, 8, 8, 8, 0, 0, size, size)
      // 帽子层（第二层）：64x64 皮肤在 (40,8)；老版 64x32 皮肤在 (32,8)。
      // 透明像素自动保留，直接叠加即可
      const hatX = img.height >= 64 ? 40 : 32
      ctx.drawImage(img, hatX, 8, 8, 8, 0, 0, size, size)
    }
    img.src = texture
  }, [texture, size])

  if (!texture) {
    return (
      <span
        style={{ width: size, height: size, fontSize: size * 0.42 }}
        className="flex shrink-0 items-center justify-center rounded-lg bg-accent-soft font-medium text-accent"
        aria-hidden
      >
        {account.name.charAt(0).toUpperCase()}
      </span>
    )
  }
  return (
    <canvas
      ref={canvasRef}
      width={size}
      height={size}
      style={{ width: size, height: size }}
      className="shrink-0 rounded-lg bg-neutral-100 dark:bg-neutral-800"
      aria-label={`${account.name} 的皮肤头像`}
    />
  )
}
