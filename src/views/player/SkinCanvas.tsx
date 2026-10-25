// Minecraft 皮肤 3D 预览画布（skinview3d 封装）
// 皮肤 / 披风贴图均以 data URL 传入（经 Rust 代理下载，绕开 http 混合内容限制）

import { useEffect, useRef } from 'react'
import * as skinview3d from 'skinview3d'

export function SkinCanvas({
  skinUrl,
  capeUrl,
  showCape,
  model = 'classic',
  width = 300,
  height = 420,
}: {
  skinUrl: string
  capeUrl: string | null
  showCape: boolean
  /** 皮肤模型：classic（粗壮）/ slim（纤细） */
  model?: string
  width?: number
  height?: number
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const viewerRef = useRef<skinview3d.SkinViewer | null>(null)

  // 初始化（仅一次；皮肤 / 披风变化经下方 effect 增量加载）
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const viewer = new skinview3d.SkinViewer({
      canvas,
      width,
      height,
      skin: skinUrl,
    })
    viewer.autoRotate = true
    viewer.autoRotateSpeed = 1.4
    viewer.animation = new skinview3d.WalkingAnimation()
    viewer.animation.speed = 0.5
    viewer.controls.enableZoom = true
    viewerRef.current = viewer
    return () => {
      viewer.dispose()
      viewerRef.current = null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // 皮肤变化（带模型：skinview3d 枚举为 default（粗壮）/ slim（纤细））
  useEffect(() => {
    viewerRef.current
      ?.loadSkin(skinUrl, { model: model === 'slim' ? 'slim' : 'default' })
      .catch(() => {})
  }, [skinUrl, model])

  // 披风变化 / 显隐切换（loadCape(null) = 移除背面装备）
  useEffect(() => {
    const viewer = viewerRef.current
    if (!viewer) return
    // skinview3d 的 loadCape 有两个重载（string / null），联合类型不匹配，分开调用
    const load = showCape && capeUrl ? viewer.loadCape(capeUrl) : viewer.loadCape(null)
    Promise.resolve(load).catch(() => {})
  }, [capeUrl, showCape])

  return <canvas ref={canvasRef} className="max-w-full" />
}
