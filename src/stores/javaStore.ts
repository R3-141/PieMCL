import { create } from 'zustand'
import { ipc } from '../lib/ipc'
import type { InstanceJavaStatus, JavaInfo } from '../types/domain'

interface JavaStoreState {
  /** 检测结果；null = 尚未检测 */
  javas: JavaInfo[] | null
  instanceStatus: InstanceJavaStatus[]
  loading: boolean
  error: string | null
  /** 重新检测（TopBar 与 Java 页共享） */
  refresh: () => Promise<void>
}

/** Java 检测结果全局缓存：TopBar 徽章、Java 管理页、编辑对话框下拉共用一份 */
export const useJavaStore = create<JavaStoreState>()((set) => ({
  javas: null,
  instanceStatus: [],
  loading: false,
  error: null,

  refresh: async () => {
    const alreadyLoading = useJavaStore.getState().loading
    if (alreadyLoading) return
    set({ loading: true, error: null })
    try {
      const r = await ipc.detectJava()
      set({ javas: r.javas, instanceStatus: r.instances, loading: false })
    } catch (e) {
      set({ error: String(e), loading: false })
    }
  },
}))
