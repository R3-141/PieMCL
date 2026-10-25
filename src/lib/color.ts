// 颜色工具：色卡（input type=color）的 hex 与 CSS 变量格式互转

/**
 * "#4f7cff" → "79 124 255"（空格分隔的 RGB 分量，供 rgb(var(--x)) 使用）
 * 非法输入返回 null
 */
export function hexToRgbTriple(hex: string): string | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return null
  const n = parseInt(m[1], 16)
  return `${(n >> 16) & 255} ${(n >> 8) & 255} ${n & 255}`
}
