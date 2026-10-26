import { describe, expect, it } from 'vitest'
import { fmtAvg, fmtDateTime, fmtInt, fmtPct, maskUid, relative } from '@/composables/format'
import { rarityClass } from '@/composables/useNames'
import { resolveLocale } from '@/i18n'
import en from '@/i18n/en.json'
import zhCN from '@/i18n/zh-CN.json'

function keys(obj: unknown, prefix = ''): string[] {
  if (obj === null || typeof obj !== 'object') return [prefix]
  return Object.entries(obj as Record<string, unknown>).flatMap(([k, v]) =>
    keys(v, prefix ? `${prefix}.${k}` : k),
  )
}

function placeholders(s: string): string[] {
  return [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]!).sort()
}

describe('语言包', () => {
  it('中英文的键完全一致', () => {
    expect(keys(en).sort()).toEqual(keys(zhCN).sort())
  })

  it('中英文的占位符一致', () => {
    const get = (o: unknown, path: string) =>
      path.split('.').reduce<unknown>((x, k) => (x as Record<string, unknown>)[k], o) as string
    for (const k of keys(zhCN)) {
      expect(placeholders(get(en, k)), k).toEqual(placeholders(get(zhCN, k)))
    }
  })

  it('跟随系统时按系统语言选择', () => {
    expect(resolveLocale('system', 'zh-TW')).toBe('zh-CN')
    expect(resolveLocale('system', 'en-US')).toBe('en')
    expect(resolveLocale('en', 'zh-CN')).toBe('en')
  })
})

describe('格式化', () => {
  it('数字、百分比与平均抽数', () => {
    expect(fmtInt(12345)).toBe('12,345')
    expect(fmtInt(null)).toBe('—')
    expect(fmtPct(0.4567)).toBe('45.7%')
    expect(fmtPct(null)).toBe('—')
    expect(fmtAvg(62.25)).toBe('62.3')
  })

  it('按服务器时区显示时间', () => {
    expect(fmtDateTime(Date.UTC(2026, 9, 24, 8, 12), 480)).toBe('2026-10-24 16:12')
    expect(fmtDateTime(Date.UTC(2026, 9, 24, 8, 12), -300)).toBe('2026-10-24 03:12')
  })

  it('相对时间', () => {
    const now = Date.UTC(2026, 9, 26, 12)
    expect(relative(now - 10_000, now)).toEqual({ key: 'time.justNow' })
    expect(relative(now - 5 * 60_000, now)).toEqual({ key: 'time.minutes', n: 5 })
    expect(relative(now - 3 * 3_600_000, now)).toEqual({ key: 'time.hours', n: 3 })
    expect(relative(now - 30 * 3_600_000, now)).toEqual({ key: 'time.yesterday' })
    expect(relative(now - 5 * 86_400_000, now)).toEqual({ key: 'time.days', n: 5 })
  })

  it('UID 打码', () => {
    expect(maskUid('100000001')).toBe('1****0001')
    expect(maskUid('1234')).toBe('1234')
  })

  it('星级颜色按游戏最高星换算', () => {
    expect(rarityClass(5, 5)).toBe('r-top')
    expect(rarityClass(4, 5)).toBe('r-second')
    expect(rarityClass(6, 6)).toBe('r-top')
    expect(rarityClass(4, 6)).toBe('r-third')
    expect(rarityClass(3, 6)).toBe('r-low')
  })
})
