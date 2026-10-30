// 单卡池的指标卡（UI/UX 文档 3.3 表格）：第二、三张卡按卡池规则选择。统计页与分享图共用。
import type { PoolInfo, PoolStatsDto } from '@/bindings'
import { fmtAvg, fmtPct } from './format'

type Translate = (key: string, args?: Record<string, unknown>) => string

export interface MetricCard {
  key: string
  label: string
  value: string
  sub: string
  /** 进度条宽度（百分比字符串）。 */
  bar?: string
  estimated: boolean
}

/** `n / of` 的百分比宽度，最多 100%。 */
export function pctOf(n: number, of: number | null | undefined) {
  return `${Math.min(100, (n / Math.max(1, of ?? 1)) * 100)}%`
}

export function poolCards(p: PoolStatsDto, info: PoolInfo | undefined, t: Translate, locale: string) {
  const hasUp = info?.up && info.up !== 'none'
  const second: MetricCard = hasUp
    ? {
        key: 'avgUp',
        label: t('stats.avgUp'),
        value: fmtAvg(p.avgPullsPerUp, locale),
        sub: t('stats.upCount', { n: p.upCount }),
        estimated: p.estimatedCount > 0,
      }
    : {
        key: 'avgTop',
        label: t('stats.avgTop'),
        value: fmtAvg(p.avgPullsPerTop, locale),
        sub: t('stats.topCountSub', { n: p.tops.length }),
        estimated: false,
      }
  let third: MetricCard
  if (info?.spark) {
    third = {
      key: 'spark',
      label: t('stats.spark'),
      value: `${p.pity.bannerPulls} / ${info.spark}`,
      sub: t('stats.sparkSub', { n: p.pity.bannerPulls }),
      bar: pctOf(p.pity.bannerPulls, info.spark),
      estimated: false,
    }
  } else if (info?.upWithin) {
    const since = p.pity.sinceUp ?? 0
    third = {
      key: 'upWithin',
      label: t('stats.upWithin', { n: info.upWithin }),
      value: `${since} / ${info.upWithin}`,
      sub: t('stats.upWithinSub', { n: since }),
      bar: pctOf(since, info.upWithin),
      estimated: p.estimatedCount > 0,
    }
  } else {
    third = {
      key: 'loss',
      label: t('stats.lossRate'),
      value: fmtPct(p.loss?.rate, locale),
      sub: p.loss ? t('stats.lossFrac', { lost: p.loss.lost, total: p.loss.total }) : '—',
      estimated: p.estimatedCount > 0,
    }
  }
  return { second, third, lossReplaced: third.key !== 'loss' && !!p.loss }
}
