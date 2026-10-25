// 时间显示辅助

/** epoch 秒 → 中文相对时间（"3 天前"）；null → "从未" */
export function formatRelativeTime(epochSec: number | null): string {
  if (epochSec === null) return '从未'
  const diff = Math.max(0, Date.now() - epochSec * 1000)
  const minutes = Math.floor(diff / 60_000)
  if (minutes < 1) return '刚刚'
  if (minutes < 60) return `${minutes} 分钟前`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours} 小时前`
  const days = Math.floor(hours / 24)
  if (days < 30) return `${days} 天前`
  const d = new Date(epochSec * 1000)
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`
}

/** epoch 秒 → 本地日期时间（tooltip 用） */
export function formatFullTime(epochSec: number): string {
  const d = new Date(epochSec * 1000)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}
