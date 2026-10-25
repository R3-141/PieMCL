// 社区资源类型配置：模组 / 数据包 / 光影包 / 资源包共用一套视图，
// 差异全部收敛在此（落位目录、来源、启停、导出、文案与分类表）。
// Rust 侧对应 src-tauri/src/mods/mod.rs 的 ResourceType。

import type { ComponentType } from 'react'
import {
  IconDatapack,
  IconMods,
  IconResourcePacks,
  IconShaders,
} from '../../components/common/Icons'

export type ResourceKind = 'mods' | 'datapacks' | 'shaders' | 'resourcepacks'

export interface CategoryOption {
  value: string
  label: string
}

export interface ResourceConfig {
  kind: ResourceKind
  /** Rust command 的 resource_type 参数 */
  resourceType: string
  /** 页面标题 / 通用称谓 */
  title: string
  icon: ComponentType<{ className?: string }>
  /** 落位目录名（文案提示用） */
  dirName: string
  /** 本地安装 / 拖放接受的扩展名 */
  acceptExt: string
  /** 启用/禁用开关（仅模组） */
  supportsToggle: boolean
  /** CurseForge 来源（仅模组） */
  supportsCurseforge: boolean
  /** .mrpack 导出（仅模组） */
  supportsExport: boolean
  /** 回收站入口（仅模组；其余类型删除即永久删除） */
  supportsTrash: boolean
  /** 展示加载器徽章与依赖警告（仅模组） */
  showsLoaderMeta: boolean
  /** 页面顶部常显提示；null 不显示 */
  notice: string | null
  /** 原版实例的顶部警示；null 不显示 */
  vanillaNote: string | null
  /** 市场环境筛选行的原版说明 */
  vanillaMarketNote: string
  /** 空态主文案 */
  emptyTitle: string
  /** 空态副文案（拖放提示） */
  emptyHint: string
}

export const RESOURCE_CONFIGS: Record<ResourceKind, ResourceConfig> = {
  mods: {
    kind: 'mods',
    resourceType: 'mod',
    title: '模组',
    icon: IconMods,
    dirName: 'mods',
    acceptExt: '.jar',
    supportsToggle: true,
    supportsCurseforge: true,
    supportsExport: true,
    supportsTrash: true,
    showsLoaderMeta: true,
    notice: null,
    vanillaNote:
      '当前实例为原版（未安装加载器），模组文件不会在游戏中生效。可先在实例「更多」中安装 Fabric / Forge。',
    vanillaMarketNote: '原版（需先安装加载器）',
    emptyTitle: '尚未安装任何模组',
    emptyHint: '将 .jar 文件拖入窗口，即可安装到当前选中的实例',
  },
  datapacks: {
    kind: 'datapacks',
    resourceType: 'datapack',
    title: '数据包',
    icon: IconDatapack,
    dirName: 'datapacks',
    acceptExt: '.zip',
    supportsToggle: false,
    supportsCurseforge: false,
    supportsExport: false,
    supportsTrash: false,
    showsLoaderMeta: false,
    notice:
      '数据包安装到实例 datapacks/ 根目录；如需对单个世界启用，请在游戏中将其移入对应世界设置。',
    vanillaNote: null,
    vanillaMarketNote: '原版可用',
    emptyTitle: '尚未安装任何数据包',
    emptyHint: '将 .zip 文件拖入窗口，即可安装到当前选中的实例的 datapacks/ 目录',
  },
  shaders: {
    kind: 'shaders',
    resourceType: 'shader',
    title: '光影包',
    icon: IconShaders,
    dirName: 'shaderpacks',
    acceptExt: '.zip',
    supportsToggle: false,
    supportsCurseforge: false,
    supportsExport: false,
    supportsTrash: false,
    showsLoaderMeta: false,
    notice: '光影需要安装 Iris（Fabric / Quilt / NeoForge）或 OptiFine 才能在游戏中生效。',
    vanillaNote: null,
    vanillaMarketNote: '原版可用（需 OptiFine）',
    emptyTitle: '尚未安装任何光影包',
    emptyHint: '将 .zip 文件拖入窗口，即可安装到当前选中的实例的 shaderpacks/ 目录',
  },
  resourcepacks: {
    kind: 'resourcepacks',
    resourceType: 'resource_pack',
    title: '资源包',
    icon: IconResourcePacks,
    dirName: 'resourcepacks',
    acceptExt: '.zip',
    supportsToggle: false,
    supportsCurseforge: false,
    supportsExport: false,
    supportsTrash: false,
    showsLoaderMeta: false,
    notice: null,
    vanillaNote: null,
    vanillaMarketNote: '原版可用',
    emptyTitle: '尚未安装任何资源包',
    emptyHint: '将 .zip 文件拖入窗口，即可安装到当前选中的实例的 resourcepacks/ 目录',
  },
}

/** 各类型的 Modrinth 内容分类（类型筛选下拉；value 为空 = 全部） */
const ALL: CategoryOption = { value: '', label: '全部类型' }

const MOD_CATEGORIES: CategoryOption[] = [
  { value: 'optimization', label: '性能优化' },
  { value: 'technology', label: '科技' },
  { value: 'magic', label: '魔法' },
  { value: 'adventure', label: '冒险' },
  { value: 'utility', label: '实用' },
  { value: 'decoration', label: '装饰' },
  { value: 'library', label: '支持库' },
  { value: 'worldgen', label: '世界生成' },
  { value: 'mobs', label: '生物' },
  { value: 'equipment', label: '装备' },
  { value: 'food', label: '食物' },
  { value: 'storage', label: '存储' },
  { value: 'transportation', label: '运输' },
  { value: 'game-mechanics', label: '游戏机制' },
  { value: 'economy', label: '经济' },
  { value: 'social', label: '社交' },
  { value: 'minigame', label: '小游戏' },
  { value: 'management', label: '管理' },
  { value: 'education', label: '教育' },
  { value: 'cursed', label: '猎奇' },
]

const DATAPACK_CATEGORIES: CategoryOption[] = [
  { value: 'adventure', label: '冒险' },
  { value: 'decoration', label: '装饰' },
  { value: 'education', label: '教育' },
  { value: 'economy', label: '经济' },
  { value: 'equipment', label: '装备' },
  { value: 'food', label: '食物' },
  { value: 'game-mechanics', label: '游戏机制' },
  { value: 'magic', label: '魔法' },
  { value: 'mobs', label: '生物' },
  { value: 'optimization', label: '性能优化' },
  { value: 'social', label: '社交' },
  { value: 'storage', label: '存储' },
  { value: 'technology', label: '科技' },
  { value: 'transportation', label: '运输' },
  { value: 'utility', label: '实用' },
  { value: 'worldgen', label: '世界生成' },
]

const SHADER_CATEGORIES: CategoryOption[] = [
  { value: 'cartoon', label: '卡通' },
  { value: 'fantasy', label: '奇幻' },
  { value: 'realistic', label: '写实' },
  { value: 'semi-realistic', label: '半写实' },
  { value: 'vanilla-like', label: '原版风' },
  { value: 'cursed', label: '猎奇' },
]

const RESOURCEPACK_CATEGORIES: CategoryOption[] = [
  { value: 'aesthetic', label: '美学' },
  { value: 'audio', label: '音频' },
  { value: 'blocks', label: '方块' },
  { value: 'combat', label: '战斗' },
  { value: 'decoration', label: '装饰' },
  { value: 'environment', label: '环境' },
  { value: 'equipment', label: '装备' },
  { value: 'fonts', label: '字体' },
  { value: 'gui', label: '界面' },
  { value: 'icons', label: '图标' },
  { value: 'items', label: '物品' },
  { value: 'mobs', label: '生物' },
  { value: 'models', label: '模型' },
  { value: 'optimization', label: '性能优化' },
  { value: 'realistic', label: '写实' },
  { value: 'simplified', label: '简化' },
  { value: 'sound', label: '音效' },
  { value: 'vanilla-like', label: '原版风' },
  { value: '8x', label: '8x 分辨率' },
  { value: '16x', label: '16x 分辨率' },
  { value: '32x', label: '32x 分辨率' },
  { value: '64x', label: '64x 分辨率' },
  { value: '128x', label: '128x 分辨率' },
  { value: '256x', label: '256x 分辨率' },
  { value: '512x', label: '512x 分辨率' },
]

export const CATEGORIES_BY_KIND: Record<ResourceKind, CategoryOption[]> = {
  mods: [ALL, ...MOD_CATEGORIES],
  datapacks: [ALL, ...DATAPACK_CATEGORIES],
  shaders: [ALL, ...SHADER_CATEGORIES],
  resourcepacks: [ALL, ...RESOURCEPACK_CATEGORIES],
}

/** 搜索结果分类徽章的中文映射（合并全部类型 + 加载器类目） */
export const CATEGORY_LABELS: Record<string, string> = Object.fromEntries(
  [...MOD_CATEGORIES, ...DATAPACK_CATEGORIES, ...SHADER_CATEGORIES, ...RESOURCEPACK_CATEGORIES].map(
    (c) => [c.value, c.label],
  ),
)
