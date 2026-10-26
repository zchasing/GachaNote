// 数字与时间格式（UI/UX 文档第 6 节）：千分位、百分比与平均抽数保留 1 位小数。

export function fmtInt(n: number | null | undefined, locale = 'zh-CN'): string {
  if (n === null || n === undefined) return '—'
  return n.toLocaleString(locale)
}

export function fmtPct(rate: number | null | undefined, locale = 'zh-CN'): string {
  if (rate === null || rate === undefined) return '—'
  return `${(rate * 100).toLocaleString(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 })}%`
}

export function fmtAvg(n: number | null | undefined, locale = 'zh-CN'): string {
  if (n === null || n === undefined) return '—'
  return n.toLocaleString(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 })
}

const pad = (n: number) => String(n).padStart(2, '0')

/** 按服务器时区显示：`2026-10-24 16:12`。 */
export function fmtDateTime(ms: number, tzOffsetMin = 480): string {
  const d = new Date(ms + tzOffsetMin * 60_000)
  return `${fmtDateParts(d)} ${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`
}

/** 按服务器时区显示日期：`2026-10-24`。 */
export function fmtDate(ms: number, tzOffsetMin = 480): string {
  return fmtDateParts(new Date(ms + tzOffsetMin * 60_000))
}

function fmtDateParts(d: Date): string {
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`
}

/** 本地时区的完整时间，用于悬停提示。 */
export function fmtLocal(ms: number): string {
  const d = new Date(ms)
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

export type RelativeUnit =
  | { key: 'time.justNow' }
  | { key: 'time.minutes' | 'time.hours' | 'time.days'; n: number }
  | { key: 'time.yesterday' }

/** 相对时间：「3 小时前」。返回语言包键与参数，由调用方翻译。 */
export function relative(ms: number, now = Date.now()): RelativeUnit {
  const diff = Math.max(0, now - ms)
  const min = Math.floor(diff / 60_000)
  if (min < 1) return { key: 'time.justNow' }
  if (min < 60) return { key: 'time.minutes', n: min }
  const h = Math.floor(min / 60)
  if (h < 24) return { key: 'time.hours', n: h }
  const days = Math.floor(h / 24)
  if (days === 1) return { key: 'time.yesterday' }
  return { key: 'time.days', n: days }
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

/** UID 中间打码，用于总览卡片：`1****6789`。 */
export function maskUid(uid: string): string {
  if (uid.length <= 5) return uid
  return `${uid.slice(0, 1)}****${uid.slice(-4)}`
}
