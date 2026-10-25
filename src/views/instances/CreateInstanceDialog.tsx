// 创建实例对话框：选 MC 版本 → 选加载器（版本下拉动态加载）→ 创建
// 创建过程展示 Rust 侧 instance-create-progress 事件进度

import { useCallback, useEffect, useState } from 'react'
import type { CreateProgress, LoaderType, McVersionEntry } from '../../types/domain'
import { ipc, onCreateProgress } from '../../lib/ipc'
import { Modal } from '../../components/common/Modal'

const LOADER_OPTIONS: readonly { value: LoaderType; label: string }[] = [
  { value: 'vanilla', label: '原版' },
  { value: 'forge', label: 'Forge' },
  { value: 'fabric', label: 'Fabric' },
  { value: 'neoforge', label: 'NeoForge' },
  { value: 'quilt', label: 'Quilt' },
]

/** 各加载器创建完成后的说明文案 */
const LOADER_HINTS: Partial<Record<LoaderType, string>> = {
  forge: '创建后将立即下载安装（含补丁处理器，需要几分钟），完成后自动下载游戏资源',
  neoforge: '创建后将立即下载安装（含补丁处理器，需要几分钟），完成后自动下载游戏资源',
  fabric: '创建后将立即下载游戏资源，加载器库随资源一并下载',
  quilt: '创建后将立即下载游戏资源，加载器库随资源一并下载',
}

const inputCls =
  'w-full rounded-lg border border-neutral-200 bg-transparent px-3 py-2 text-sm outline-none transition-colors duration-150 focus:border-accent dark:border-neutral-700'
const labelCls = 'mb-1.5 block text-sm font-medium'

interface CreateInstanceDialogProps {
  open: boolean
  onClose: () => void
}

export function CreateInstanceDialog({ open, onClose }: CreateInstanceDialogProps) {
  const [name, setName] = useState('')
  const [versions, setVersions] = useState<McVersionEntry[] | null>(null)
  const [versionsError, setVersionsError] = useState<string | null>(null)
  const [showSnapshots, setShowSnapshots] = useState(false)
  const [mcVersion, setMcVersion] = useState('')
  const [loaderType, setLoaderType] = useState<LoaderType>('vanilla')
  const [loaderVersions, setLoaderVersions] = useState<string[] | null>(null)
  const [loaderVersion, setLoaderVersion] = useState('')
  // Fabric / Quilt 附加组件
  const [withFabricApi, setWithFabricApi] = useState(true)
  const [withIrisSodium, setWithIrisSodium] = useState(false)
  // Forge 附加组件：OptiFine 功能开发中（勾选 UI 已置灰，Rust 安装代码保留）
  const [creating, setCreating] = useState(false)
  const [progress, setProgress] = useState<CreateProgress | null>(null)
  const [error, setError] = useState<string | null>(null)

  const loadVersions = useCallback(async (snapshots: boolean, refresh: boolean) => {
    setVersionsError(null)
    try {
      const list = await ipc.listMcVersions(snapshots, refresh)
      setVersions(list)
      setMcVersion((cur) => (cur && list.some((v) => v.id === cur) ? cur : (list[0]?.id ?? '')))
    } catch (e) {
      setVersionsError(String(e))
    }
  }, [])

  // 打开时加载版本清单
  useEffect(() => {
    if (open && versions === null) loadVersions(false, false)
  }, [open, versions, loadVersions])

  // MC 版本 / 加载器变化 → 动态加载加载器版本
  useEffect(() => {
    if (!open || loaderType === 'vanilla' || !mcVersion) {
      setLoaderVersions(null)
      setLoaderVersion('')
      return
    }
    let cancelled = false
    setLoaderVersions(null)
    setLoaderVersion('')
    ipc
      .listLoaderVersions(loaderType, mcVersion)
      .then((list) => {
        if (cancelled) return
        setLoaderVersions(list)
        setLoaderVersion(list[0] ?? '')
      })
      .catch((e) => {
        if (!cancelled) setLoaderVersions([])
        if (!cancelled) setError(`获取加载器版本失败：${e}`)
      })
    return () => {
      cancelled = true
    }
  }, [open, loaderType, mcVersion])

  // 创建期间监听进度事件
  useEffect(() => {
    if (!creating) return
    let unlisten: (() => void) | undefined
    onCreateProgress(setProgress).then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [creating])

  const needLoaderVersion = loaderType !== 'vanilla'
  const canSubmit =
    !creating &&
    name.trim().length > 0 &&
    mcVersion.length > 0 &&
    (!needLoaderVersion || (loaderVersions !== null && loaderVersions.length > 0))

  const handleSubmit = async () => {
    setCreating(true)
    setError(null)
    setProgress(null)
    try {
      const summary = await ipc.createInstance({
        name: name.trim(),
        mcVersion,
        loaderType,
        loaderVersion: needLoaderVersion ? loaderVersion || undefined : undefined,
      })
      // 先记下触发参数（关窗后组件状态不再可用）
      const { id } = summary
      const lt = loaderType
      const lv = loaderVersion || undefined
      const wantApi = withFabricApi
      const wantIris = withIrisSodium
      // 创建成功立即关窗（Forge 安装耗时数分钟，不能阻塞对话框）；
      // 安装/下载转后台任务，进度在下载面板展示
      handleClose()
      void (async () => {
        try {
          if (lt === 'forge' || lt === 'neoforge') {
            await ipc.installLoader(id, lt, lv)
          } else {
            await ipc.installGameResources(id)
            if ((lt === 'fabric' || lt === 'quilt') && (wantApi || wantIris)) {
              // mods 目录与游戏资源互不冲突，可并行下载
              await ipc
                .installFabricAddons(id, wantApi, wantIris)
                .catch(() => {
                  /* 附加组件失败不阻塞：资源管理页可手动重装 */
                })
            }
          }
        } catch {
          /* 后台触发失败静默：可稍后手动安装，或启动前检查自动补全 */
        }
      })()
    } catch (e) {
      setError(String(e))
    } finally {
      setCreating(false)
    }
  }

  const handleClose = () => {
    if (creating) return
    setName('')
    setLoaderType('vanilla')
    setLoaderVersions(null)
    setLoaderVersion('')
    setWithFabricApi(true)
    setWithIrisSodium(false)
    setError(null)
    setProgress(null)
    onClose()
  }

  return (
    <Modal open={open} onClose={handleClose} title="新建实例" maxWidth="max-w-lg">
      <div className="space-y-4">
        <div>
          <label className={labelCls} htmlFor="ci-name">实例名称</label>
          <input
            id="ci-name"
            className={inputCls}
            placeholder="例如：1.21 生存"
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={creating}
          />
        </div>

        <div>
          <div className="flex items-center justify-between">
            <label className={labelCls} htmlFor="ci-mc">游戏版本</label>
            <div className="flex items-center gap-3">
              <label className="flex cursor-pointer items-center gap-1.5 text-xs text-neutral-500 dark:text-neutral-400">
                <input
                  type="checkbox"
                  className="size-3.5 accent-[var(--accent)]"
                  checked={showSnapshots}
                  onChange={(e) => {
                    setShowSnapshots(e.target.checked)
                    loadVersions(e.target.checked, false)
                  }}
                  disabled={creating}
                />
                显示快照版
              </label>
              <button
                type="button"
                className="text-xs text-accent hover:underline disabled:opacity-50"
                onClick={() => loadVersions(showSnapshots, true)}
                disabled={creating}
              >
                刷新
              </button>
            </div>
          </div>
          {versionsError ? (
            <p className="text-sm text-red-500">{versionsError}</p>
          ) : versions === null ? (
            <p className="text-sm text-neutral-400">正在加载版本清单…</p>
          ) : (
            <select
              id="ci-mc"
              className={inputCls}
              value={mcVersion}
              onChange={(e) => setMcVersion(e.target.value)}
              disabled={creating}
            >
              {versions.map((v) => (
                <option key={v.id} value={v.id}>
                  {v.id}{v.versionType !== 'release' ? `（${v.versionType}）` : ''}
                </option>
              ))}
            </select>
          )}
        </div>

        <div>
          <label className={labelCls} htmlFor="ci-loader">模组加载器</label>
          <select
            id="ci-loader"
            className={inputCls}
            value={loaderType}
            onChange={(e) => setLoaderType(e.target.value as LoaderType)}
            disabled={creating}
          >
            {LOADER_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>{o.label}</option>
            ))}
          </select>
          {needLoaderVersion && (
            <p className="mt-1 text-xs text-neutral-400 dark:text-neutral-500">
              {LOADER_HINTS[loaderType]}
            </p>
          )}
        </div>

        {needLoaderVersion && (
          <div>
            <label className={labelCls} htmlFor="ci-loader-ver">加载器版本</label>
            {loaderVersions === null ? (
              <p className="text-sm text-neutral-400">正在加载…</p>
            ) : loaderVersions.length === 0 ? (
              <p className="text-sm text-red-500">该游戏版本暂无可用加载器版本</p>
            ) : (
              <select
                id="ci-loader-ver"
                className={inputCls}
                value={loaderVersion}
                onChange={(e) => setLoaderVersion(e.target.value)}
                disabled={creating}
              >
                {loaderVersions.map((v) => (
                  <option key={v} value={v}>{v}</option>
                ))}
              </select>
            )}
          </div>
        )}

        {(loaderType === 'fabric' || loaderType === 'quilt') && (
          <div>
            <label className={labelCls}>附加组件</label>
            <label className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:hover:bg-neutral-800">
              <input
                type="checkbox"
                className="size-4 accent-[var(--accent)]"
                checked={withFabricApi}
                onChange={(e) => setWithFabricApi(e.target.checked)}
                disabled={creating}
              />
              Fabric API
              <span className="text-xs text-neutral-400 dark:text-neutral-500">多数模组的前置</span>
            </label>
            <label className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm transition-colors duration-150 hover:bg-neutral-100 dark:hover:bg-neutral-800">
              <input
                type="checkbox"
                className="size-4 accent-[var(--accent)]"
                checked={withIrisSodium}
                onChange={(e) => setWithIrisSodium(e.target.checked)}
                disabled={creating}
              />
              Iris + Sodium
              <span className="text-xs text-neutral-400 dark:text-neutral-500">
                光影与画质优化（所需前置自动一并安装）
              </span>
            </label>
          </div>
        )}

        {(loaderType === 'forge') && (
          <div>
            <label className={labelCls}>附加组件</label>
            {/* OptiFine：功能开发中，勾选禁用（Rust 侧安装代码保留，恢复仅需解除此处置灰） */}
            <label className="flex cursor-not-allowed items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm opacity-50">
              <input
                type="checkbox"
                className="size-4 accent-[var(--accent)]"
                checked={false}
                disabled
              />
              OptiFine
              <span className="text-xs text-amber-500">功能开发中</span>
              <span className="text-xs text-neutral-400 dark:text-neutral-500">
                高清修复（开发完成后以官方兼容方式与 Forge 共存）
              </span>
            </label>
          </div>
        )}

        {creating && (
          <p className="text-sm text-accent">{progress?.message ?? '正在创建…'}</p>
        )}
        {error && <p className="text-sm text-red-500">{error}</p>}

        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={handleClose}
            disabled={creating}
            className="rounded-lg px-4 py-2 text-sm text-neutral-600 transition-colors duration-150 hover:bg-neutral-100 disabled:opacity-50 dark:text-neutral-300 dark:hover:bg-neutral-800"
          >
            取消
          </button>
          <button
            type="button"
            onClick={handleSubmit}
            disabled={!canSubmit}
            className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition-colors duration-150 hover:bg-accent-hover disabled:opacity-50"
          >
            {creating ? '创建中…' : '创建'}
          </button>
        </div>
      </div>
    </Modal>
  )
}
