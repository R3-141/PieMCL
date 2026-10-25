// 手写 stroke 风格 SVG 图标（零依赖，currentColor 跟随文字颜色）
interface IconProps {
  className?: string
}

function Svg({ className, children }: IconProps & { children: React.ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {children}
    </svg>
  )
}

/** 实例：立方体 */
export function IconInstances({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z" />
      <path d="M3.27 6.96 12 12.01l8.73-5.05" />
      <path d="M12 22.08V12" />
    </Svg>
  )
}

/** 下载：向下箭头入盘 */
export function IconHome({ className }: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className={className}>
      <path d="M3 10.5 12 3l9 7.5" />
      <path d="M5 9.5V21h14V9.5" />
      <path d="M10 21v-6h4v6" />
    </svg>
  )
}

export function IconDownloads({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <path d="M7 10l5 5 5-5" />
      <path d="M12 15V3" />
    </Svg>
  )
}

/** 窗口控制：最小化（横线） */
export function IconMinus({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M5 12h14" />
    </Svg>
  )
}

/** 窗口控制：最大化（方框） */
export function IconMaximize({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="5" y="5" width="14" height="14" rx="1.5" />
    </Svg>
  )
}

/** 日志：终端窗口 */
export function IconLogs({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M7 9l3 3-3 3" />
      <path d="M13 15h4" />
    </Svg>
  )
}

/** 玩家：人形 */
export function IconPlayer({ className }: IconProps) {
  return (
    <Svg className={className}>
      <circle cx="12" cy="8" r="4" />
      <path d="M5 21c0-3.9 3.1-7 7-7s7 3.1 7 7" />
    </Svg>
  )
}

/** 模组：四格 + 加号 */
export function IconMods({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <path d="M17.5 14v7" />
      <path d="M14 17.5h7" />
    </Svg>
  )
}

/** 社区资源（父级分组）：积木堆叠 */
export function IconCommunity({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="3" y="13.5" width="8" height="7.5" rx="1.5" />
      <rect x="13" y="13.5" width="8" height="7.5" rx="1.5" />
      <rect x="8" y="3" width="8" height="7.5" rx="1.5" />
    </Svg>
  )
}

/** 数据包：层叠数据带 */
export function IconDatapack({ className }: IconProps) {
  return (
    <Svg className={className}>
      <ellipse cx="12" cy="5.5" rx="8" ry="2.8" />
      <path d="M4 5.5v6c0 1.55 3.58 2.8 8 2.8s8-1.25 8-2.8v-6" />
      <path d="M4 11.5v6c0 1.55 3.58 2.8 8 2.8s8-1.25 8-2.8v-6" />
    </Svg>
  )
}

/** 光影包：光芒方块 */
export function IconShaders({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="7" y="7" width="10" height="10" rx="2" />
      <path d="M12 2.5v2.5" />
      <path d="M12 19v2.5" />
      <path d="M2.5 12H5" />
      <path d="M19 12h2.5" />
      <path d="M5.3 5.3l1.8 1.8" />
      <path d="M16.9 16.9l1.8 1.8" />
      <path d="M18.7 5.3l-1.8 1.8" />
      <path d="M7.1 16.9l-1.8 1.8" />
    </Svg>
  )
}

/** 资源包：图片画框（山 + 太阳） */
export function IconResourcePacks({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <circle cx="9" cy="10" r="1.6" />
      <path d="M4 17l4.5-4.5 3 3L15 12l5 5" />
    </Svg>
  )
}

/** 设置：齿轮 */
export function IconSettings({ className }: IconProps) {
  return (
    <Svg className={className}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </Svg>
  )
}

/** 折叠：双尖括号 */
export function IconChevrons({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M11 17l-5-5 5-5" />
      <path d="M18 17l-5-5 5-5" />
    </Svg>
  )
}

/** Java 状态：咖啡杯 */
export function IconJava({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M18 8h1a4 4 0 0 1 0 8h-1" />
      <path d="M2 8h16v9a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V8z" />
      <path d="M6 1v3" />
      <path d="M10 1v3" />
      <path d="M14 1v3" />
    </Svg>
  )
}

/** 关闭 */
export function IconClose({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M18 6 6 18" />
      <path d="M6 6l12 12" />
    </Svg>
  )
}

/** 启动：三角播放 */
export function IconPlay({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M6 4.5v15a1 1 0 0 0 1.52.86l12-7.5a1 1 0 0 0 0-1.72l-12-7.5A1 1 0 0 0 6 4.5z" />
    </Svg>
  )
}

/** 更多：水平三点 */
export function IconMore({ className }: IconProps) {
  return (
    <Svg className={className}>
      <circle cx="5" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="19" cy="12" r="1.4" fill="currentColor" stroke="none" />
    </Svg>
  )
}

/** 文件夹 */
export function IconFolder({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7z" />
    </Svg>
  )
}

/** 加号 */
export function IconPlus({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </Svg>
  )
}

/** 相机（截图选项卡） */
export function IconCamera({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M4 8a2 2 0 0 1 2-2h2l1.5-2h5L16 6h2a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8z" />
      <circle cx="12" cy="13" r="3.5" />
    </Svg>
  )
}

/** 档案盒（存档选项卡） */
export function IconArchive({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M4 5h16v4H4z" />
      <path d="M6 9v10h12V9" />
      <path d="M10 13h4" />
    </Svg>
  )
}

/** 调节滑杆（游戏配置页） */
export function IconConfig({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M4 8h10M18 8h2" />
      <circle cx="16" cy="8" r="2" />
      <path d="M4 16h2M10 16h10" />
      <circle cx="8" cy="16" r="2" />
    </Svg>
  )
}

/** 服务器机架（服务器页） */
export function IconServer({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="4" y="4" width="16" height="6" rx="1.5" />
      <rect x="4" y="14" width="16" height="6" rx="1.5" />
      <path d="M7.5 7h.01M7.5 17h.01" />
      <path d="M13 7h3M13 17h3" />
    </Svg>
  )
}

/** 联机（内网穿透，地球 + 链路） */
export function IconLan({ className }: IconProps) {
  return (
    <Svg className={className}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M3.5 12h17" />
      <path d="M12 3.5c2.5 2.3 3.8 5.2 3.8 8.5s-1.3 6.2-3.8 8.5c-2.5-2.3-3.8-5.2-3.8-8.5S9.5 5.8 12 3.5Z" />
    </Svg>
  )
}

/** 外部链接（方框 + 右上箭头） */
export function IconExternal({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="M14 4h6v6" />
      <path d="M20 4 11 13" />
      <path d="M18 13v6a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 4 19V8a1.5 1.5 0 0 1 1.5-1.5H11" />
    </Svg>
  )
}

/** 信息（圆 + i） */
export function IconInfo({ className }: IconProps) {
  return (
    <Svg className={className}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5" />
      <path d="M12 7.5h.01" />
    </Svg>
  )
}

/** 邮件（信封） */
export function IconMail({ className }: IconProps) {
  return (
    <Svg className={className}>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <path d="m3 7 9 6 9-6" />
    </Svg>
  )
}

/** 代码（尖括号） */
export function IconCode({ className }: IconProps) {
  return (
    <Svg className={className}>
      <path d="m8 7-5 5 5 5" />
      <path d="m16 7 5 5-5 5" />
    </Svg>
  )
}

