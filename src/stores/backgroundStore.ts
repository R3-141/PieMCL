// 自定义背景状态：图片 dataUrl 与模糊 / 遮罩 / 界面明暗参数
// 图片数据由 Rust 读文件转 data URL 一次性加载；参数修改由设置页实时同步

import { create } from 'zustand'
import { ipc } from '../lib/ipc'

interface BackgroundState {
  /** 背景图 data URL；null = 未启用 */
  dataUrl: string | null
  /** 模糊程度（px，0–40） */
  blur: number
  /** 遮罩暗度（%，0–70） */
  dim: number
  /** 背景启用时的界面明暗：auto（跟随主题）/ dark / light */
  theme: string
  /** 设置页保存图片 / 修改参数后整体同步 */
  apply: (dataUrl: string | null, blur: number, dim: number) => void
  /** 仅更新模糊 / 遮罩参数（滑块实时预览） */
  setParams: (blur: number, dim: number) => void
  /** 更新界面明暗覆盖（设置页下拉） */
  setTheme: (t: string) => void
  /** 启动时从设置加载（未启用或文件缺失保持无背景） */
  load: () => Promise<void>
}

export const useBackgroundStore = create<BackgroundState>()((set) => ({
  dataUrl: null,
  blur: 0,
  dim: 25,
  theme: 'auto',
  apply: (dataUrl, blur, dim) => set({ dataUrl, blur, dim }),
  setParams: (blur, dim) => set({ blur, dim }),
  setTheme: (t) => set({ theme: t }),
  load: async () => {
    try {
      const s = await ipc.getSettings()
      set({ theme: s.backgroundTheme || 'auto' })
      if (!s.backgroundImage) return
      const data = await ipc.readBackgroundImage()
      if (data) set({ dataUrl: data, blur: s.backgroundBlur, dim: s.backgroundDim })
    } catch {
      /* 加载失败静默保持无背景 */
    }
  },
}))
