// 图表共用的配色与坐标轴样式：颜色全部来自设计 token（UI/UX 文档第 5 节）。
import type { MonthDto } from '@/bindings'

/** 读取 CSS 变量，让图表配色跟随深浅主题。 */
export type CssVar = (name: string) => string

/** 坐标轴、网格与提示框的公共样式。 */
export function baseOption(css: CssVar) {
  const axis = {
    axisLine: { lineStyle: { color: css('--border') } },
    axisTick: { show: false },
    axisLabel: { color: css('--text2'), fontSize: 11 },
    splitLine: { lineStyle: { color: css('--track') } },
  }
  return {
    animationDuration: 300,
    textStyle: { fontFamily: css('--font') },
    grid: { left: 8, right: 16, top: 28, bottom: 8, containLabel: true },
    tooltip: {
      trigger: 'axis' as const,
      backgroundColor: css('--surface'),
      borderColor: css('--border'),
      textStyle: { color: css('--text'), fontSize: 12 },
    },
    legend: { top: 0, right: 0, textStyle: { color: css('--text2'), fontSize: 12 }, itemWidth: 12, itemHeight: 8 },
    axis,
  }
}

/** 按月趋势：柱为抽数，折线为出最高星数（分析页与时间线页「按月」共用）。 */
export function monthlyOption(css: CssVar, months: MonthDto[], names: { pulls: string; tops: string }) {
  const base = baseOption(css)
  return {
    ...base,
    xAxis: { type: 'category', data: months.map((m) => m.month), ...base.axis, splitLine: { show: false } },
    yAxis: [
      { type: 'value', ...base.axis },
      { type: 'value', ...base.axis, minInterval: 1, splitLine: { show: false } },
    ],
    series: [
      {
        name: names.pulls,
        type: 'bar',
        barMaxWidth: 24,
        itemStyle: { color: css('--bar3'), borderRadius: [3, 3, 0, 0] },
        data: months.map((m) => m.pulls),
      },
      {
        name: names.tops,
        type: 'line',
        yAxisIndex: 1,
        symbolSize: 6,
        lineStyle: { color: css('--bar5'), width: 2 },
        itemStyle: { color: css('--bar5') },
        data: months.map((m) => m.tops),
      },
    ],
  }
}

/** 欧非评级：按「比多少比例的人运气好」分五档（F-17）。 */
export type LuckTier = 'great' | 'lucky' | 'even' | 'unlucky' | 'awful'

export function luckTier(percentile: number): LuckTier {
  if (percentile >= 0.9) return 'great'
  if (percentile >= 0.65) return 'lucky'
  if (percentile >= 0.35) return 'even'
  if (percentile >= 0.1) return 'unlucky'
  return 'awful'
}
