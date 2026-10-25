// 字节与速率格式化（下载面板 / Java 页共用）

/** 字节数 → 可读文本（B / KB / MB / GB，一位小数） */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let v = n
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i += 1
  }
  return `${i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`
}

/** 字节/秒 → 可读速率（< 1 KB/s 时显示 B/s） */
export function formatSpeed(bytesPerSecond: number): string {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return '0 B/s'
  return `${formatBytes(bytesPerSecond)}/s`
}
