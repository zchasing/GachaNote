<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import type { AccountStatsDto, MonthDto, PeriodDto, PoolInfo, PoolStatsDto, TimelineDto, UpMark } from '@/bindings'
import { api } from '@/api'
import DateBrush from '@/components/DateBrush.vue'
import EChart from '@/components/EChart.vue'
import ItemIcon from '@/components/ItemIcon.vue'
import SegmentedControl from '@/components/SegmentedControl.vue'
import { baseOption, monthlyOption, type CssVar } from '@/composables/chart'
import { fmtDate, fmtInt } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useUiStore } from '@/stores/ui'

// 时间线标签（UI/UX 文档 3.6，F-19）：上面是按日星级堆叠图 + 日期选区（可切按月），
// 下面是卡池期数列表。期数的划分、抽数与出金都来自后端 get_timeline。
const route = useRoute()
const accounts = useAccountsStore()
const games = useGamesStore()
const ui = useUiStore()
const { report } = useErrors()
const { t, locale, poolName, poolShort, rarityName } = useNames()

const id = computed(() => String(route.params.id))
const account = computed(() => accounts.account(id.value))
const game = computed(() => account.value?.game ?? '')
const rarityTop = computed(() => games.game(game.value)?.rarityTop ?? 5)
const topName = computed(() => rarityName(game.value, rarityTop.value))
const tz = computed(() => account.value?.tzOffsetMin ?? 480)

const stats = computed<AccountStatsDto | undefined>(() => accounts.stats[id.value])
const loadingStats = ref(false)
watch(
  [id, () => accounts.stats[id.value]],
  async ([, s]) => {
    if (s || loadingStats.value) return
    loadingStats.value = true
    try {
      await accounts.loadStats(id.value)
    } catch (e) {
      report(e)
    } finally {
      loadingStats.value = false
    }
  },
  { immediate: true },
)

interface Row {
  stats: PoolStatsDto
  info: PoolInfo | undefined
}
const withPulls = computed<Row[]>(() =>
  (stats.value?.pools ?? [])
    .filter((p) => p.totalPulls > 0)
    .map((p) => ({ stats: p, info: games.pool(game.value, p.poolType) })),
)
// 与其他标签共用所选卡池；统计标签选「总览」时这里显示第一个卡池
const selected = computed({
  get: () => {
    const v = ui.selectedPool[id.value]
    return withPulls.value.some((r) => r.stats.poolType === v) ? v! : (withPulls.value[0]?.stats.poolType ?? '')
  },
  set: (v: string) => (ui.selectedPool = { ...ui.selectedPool, [id.value]: v }),
})
const segments = computed(() =>
  withPulls.value.map((r) => ({
    value: r.stats.poolType,
    label: poolShort(game.value, r.stats.poolType),
    badge: r.info && r.info.hard > 0 ? r.stats.pity.sinceTop : null,
  })),
)
const current = computed(() => withPulls.value.find((r) => r.stats.poolType === selected.value))
const hasUp = computed(() => !!current.value?.info && current.value.info.up !== 'none')

const timeline = ref<TimelineDto | null>(null)
const months = ref<MonthDto[]>([])
let seq = 0
watch(
  [id, selected, () => accounts.version],
  async () => {
    const pool = selected.value
    if (!pool) return
    const mine = ++seq
    try {
      const [tl, m] = await Promise.all([api.getTimeline(id.value, pool), api.getMonthlyTrend(id.value, pool)])
      if (mine !== seq) return
      timeline.value = tl
      months.value = m
    } catch (e) {
      if (mine === seq) report(e)
    }
  },
  { immediate: true },
)

// ---------- 按日图表 ----------

const mode = ref<'day' | 'month'>('day')
const modes = computed(() => [
  { value: 'day' as const, label: t('timeline.byDay') },
  { value: 'month' as const, label: t('timeline.byMonth') },
])

const daily = computed(() => timeline.value?.daily ?? null)
const days = computed(() => daily.value?.top.length ?? 0)
const totals = computed(() => {
  const d = daily.value
  if (!d) return []
  return d.top.map((v, i) => v + d.second[i]! + d.other[i]!)
})

const pad = (n: number) => String(n).padStart(2, '0')
/** 第 i 天的日期（服务器时区）。 */
function dateOf(i: number): string {
  const [y, m, d] = (daily.value?.start ?? '1970-01-01').split('-').map(Number)
  const dt = new Date(Date.UTC(y!, m! - 1, d! + i))
  return `${dt.getUTCFullYear()}-${pad(dt.getUTCMonth() + 1)}-${pad(dt.getUTCDate())}`
}

/** 选区：存的是距今天（最后一天）几天，换成下标时按当前数据的天数夹紧。 */
const range = computed<[number, number]>({
  get: (): [number, number] => {
    const n = days.value
    const r = ui.timelineRange
    if (!r || n === 0) return [0, Math.max(0, n - 1)]
    const end = Math.max(0, n - 1 - r.to)
    const start = Math.min(end, Math.max(0, n - 1 - r.from))
    return [start, end]
  },
  set: ([s, e]) => {
    const n = days.value
    ui.timelineRange = s === 0 && e === n - 1 ? null : { from: n - 1 - s, to: n - 1 - e }
  },
})

const quick = computed(() =>
  (
    [
      ['d30', 30],
      ['d90', 90],
      ['d180', 183],
      ['all', 0],
    ] as const
  ).map(([key, len]) => {
    const r = ui.timelineRange
    const on = len === 0 ? !r : !!r && r.to === 0 && r.from === len - 1
    return { key, len, on, label: t(`timeline.quick.${key}`) }
  }),
)
function pickQuick(len: number) {
  ui.timelineRange = len === 0 ? null : { from: len - 1, to: 0 }
}

/** 选区内的统计：图表标题、图例、纵轴、峰值都只按选区计算。 */
const view = computed(() => {
  const d = daily.value
  if (!d || days.value === 0) return null
  const [s, e] = range.value
  const idx = Array.from({ length: e - s + 1 }, (_, k) => s + k)
  const sum = (arr: number[]) => idx.reduce((a, i) => a + arr[i]!, 0)
  let peak = idx[0]!
  for (const i of idx) if (totals.value[i]! > totals.value[peak]!) peak = i
  return {
    idx,
    from: dateOf(s),
    to: dateOf(e),
    total: sum(totals.value),
    top: sum(d.top),
    second: sum(d.second),
    other: sum(d.other),
    peak,
  }
})

const tierNames = computed(() => {
  const top = rarityTop.value
  const third = rarityName(game.value, top - 2)
  return {
    top: topName.value,
    second: rarityName(game.value, top - 1),
    // 方舟、终末地最低到 3 星，第三档合并显示
    other: top >= 6 ? t('timeline.andBelow', { name: third }) : third,
  }
})

const legend = computed(() => {
  const v = view.value
  if (!v) return []
  return [
    { key: 'other', color: 'var(--bar3)', label: tierNames.value.other, n: v.other },
    { key: 'second', color: 'var(--bar4)', label: tierNames.value.second, n: v.second },
    { key: 'top', color: 'var(--bar5)', label: tierNames.value.top, n: v.top },
  ]
})

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`)

function dayOption(css: CssVar) {
  const v = view.value
  const d = daily.value
  if (!v || !d) return {}
  const base = baseOption(css)
  const dates = v.idx.map(dateOf)
  const series = (
    [
      ['other', d.other, '--bar3'],
      ['second', d.second, '--bar4'],
      ['top', d.top, '--bar5'],
    ] as const
  ).map(([key, arr, color], k) => ({
    name: tierNames.value[key],
    type: 'bar',
    stack: 'day',
    barCategoryGap: '20%',
    barMaxWidth: 18,
    itemStyle: { color: css(color), borderRadius: k === 2 ? [2, 2, 0, 0] : 0 },
    emphasis: { focus: 'none' },
    // 没抽的日子不画柱子，几年的数据也不会生成上千个空图形
    data: v.idx.map((i) => arr[i] || null),
    ...(k === 2 && totals.value[v.peak]! > 0
      ? {
          markPoint: {
            symbol: 'circle',
            symbolSize: 0,
            itemStyle: { color: 'transparent' },
            label: { show: true, position: 'top', color: css('--text'), fontSize: 11, formatter: '{c}' },
            data: [{ coord: [dateOf(v.peak), totals.value[v.peak]], value: totals.value[v.peak] }],
          },
        }
      : {}),
  }))
  return {
    ...base,
    legend: { show: false },
    grid: { ...base.grid, top: 20 },
    tooltip: {
      ...base.tooltip,
      axisPointer: { type: 'line', lineStyle: { type: 'dashed', color: css('--text2') } },
      // 鼠标在图表右侧 40% 时浮框显示在虚线左边
      position: (point: number[], _p: unknown, _d: unknown, _r: unknown, size: { contentSize: number[]; viewSize: number[] }) => {
        const [x, y] = point as [number, number]
        const w = size.contentSize[0]!
        return x > size.viewSize[0]! * 0.6 ? [x - w - 12, y] : [x + 12, y]
      },
      formatter: (params: { dataIndex: number }[]) => {
        const i = v.idx[params[0]?.dataIndex ?? 0]!
        const rows = legend.value
          .slice()
          .reverse()
          .map((l) => {
            const n = (d[l.key as 'top' | 'second' | 'other'] as number[])[i]
            const color = css(l.key === 'top' ? '--bar5' : l.key === 'second' ? '--bar4' : '--bar3')
            return `<div style="display:flex;justify-content:space-between;gap:16px"><span><span style="display:inline-block;width:8px;height:8px;border-radius:50%;background:${color};margin-right:6px"></span>${esc(l.label)}</span><b>${n}</b></div>`
          })
          .join('')
        return `<div style="color:${css('--text2')}">${dateOf(i)}</div><div style="display:flex;justify-content:space-between;gap:16px"><span>${esc(t('timeline.dayPulls'))}</span><b>${totals.value[i]}</b></div>${rows}`
      },
    },
    xAxis: { type: 'category', data: dates, ...base.axis, splitLine: { show: false } },
    yAxis: { type: 'value', minInterval: 1, ...base.axis },
    series,
  }
}

function monthOption(css: CssVar) {
  return monthlyOption(css, months.value, {
    pulls: t('analysis.monthPulls'),
    tops: t('analysis.monthTops', { name: topName.value }),
  })
}

const dayTable = ref(false)
const tableRows = computed(() => {
  const v = view.value
  const d = daily.value
  if (!v || !d) return []
  return v.idx
    .filter((i) => totals.value[i]! > 0)
    .reverse()
    .map((i) => ({ date: dateOf(i), total: totals.value[i]!, top: d.top[i]!, second: d.second[i]!, other: d.other[i]! }))
})

// ---------- 卡池期数 ----------

const showSkipped = ref(false)

/** 按界面语言取名称：中文用 zh-cn，英文用 en-us，缺失时依次回退。 */
function localName(names: Partial<Record<string, string>>, fallback: string) {
  const want = locale.value.startsWith('zh') ? ['zh-cn', 'zh-tw', 'en-us'] : ['en-us', 'zh-cn']
  for (const k of want) if (names[k]) return names[k]!
  return Object.values(names).find(Boolean) ?? fallback
}

interface PeriodView {
  key: string
  joined: boolean
  title: string
  upTop: string[]
  upSecond: string[]
  dates: string
  period: PeriodDto
}

const periods = computed<PeriodView[]>(() =>
  (timeline.value?.periods ?? []).map((p, i) => {
    const from = fmtDate(p.start, tz.value)
    const to = fmtDate(Math.max(p.start, p.end - 1), tz.value)
    const upTop = p.upTop.map((u) => localName(u.names, u.key))
    const banner = localName(p.names, '')
    const title =
      p.kind === 'auto'
        ? t('timeline.autoTitle', { from, to: to.slice(from.slice(0, 4) === to.slice(0, 4) ? 5 : 0) })
        : p.kind === 'pool'
          ? (p.rawName ?? p.id ?? '')
          : banner || (upTop.length ? '' : (p.id ?? ''))
    return {
      key: `${p.id ?? 'auto'}-${p.start}-${i}`,
      joined: p.pulls > 0,
      title,
      upTop,
      upSecond: p.upSecond.map((u) => localName(u.names, u.key)),
      dates: `${from} – ${to}`,
      period: p,
    }
  }),
)
const visiblePeriods = computed(() => periods.value.filter((p) => p.joined || showSkipped.value))
const hasAuto = computed(() => periods.value.some((p) => p.period.kind === 'auto'))

function markClass(up: UpMark | null) {
  return up === 'up' ? 'tag up' : up === 'notUp' ? 'tag loss' : 'tag neutral'
}
function markText(up: UpMark | null) {
  return up === 'up' ? t('stats.up') : up === 'notUp' ? t('stats.lost') : t('stats.unknown')
}
</script>

<template>
  <div class="content">
    <div class="wrap">
      <div v-if="!stats" class="skeleton" style="height: 320px" />
      <div v-else-if="withPulls.length === 0" class="empty">{{ t('stats.noData') }}</div>
      <template v-else-if="current">
        <SegmentedControl v-model="selected" :options="segments" :label="t('records.pool')" />

        <div class="card" data-test="timeline-chart">
          <div class="row" style="justify-content: space-between">
            <div class="ct">{{ t('timeline.chart') }}</div>
            <div class="row" style="gap: 12px">
              <button v-if="mode === 'day'" class="linkbtn" data-test="toggle-day-table" @click="dayTable = !dayTable">
                {{ dayTable ? t('timeline.chartView') : t('timeline.table') }}
              </button>
              <SegmentedControl v-model="mode" :options="modes" :label="t('timeline.granularity')" />
            </div>
          </div>

          <template v-if="mode === 'day'">
            <div v-if="!view" class="muted">{{ t('timeline.empty') }}</div>
            <template v-else>
              <div class="row" style="justify-content: space-between">
                <div class="num" style="font-weight: 600" data-test="day-range">
                  {{ view.from }} – {{ view.to }}<span class="muted" style="font-weight: 400">{{
                    t('timeline.total', { n: fmtInt(view.total, locale) })
                  }}</span>
                </div>
                <div class="legend">
                  <span v-for="l in legend" :key="l.key">
                    <span class="sw" :style="{ background: l.color }" />{{ l.label }}（{{ l.n }}）
                  </span>
                </div>
              </div>
              <div v-if="dayTable" class="tablebox minitable" data-test="day-table">
                <table>
                  <thead>
                    <tr>
                      <th>{{ t('timeline.date') }}</th>
                      <th>{{ t('timeline.dayPulls') }}</th>
                      <th>{{ tierNames.top }}</th>
                      <th>{{ tierNames.second }}</th>
                      <th>{{ tierNames.other }}</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="r in tableRows" :key="r.date">
                      <td class="num">{{ r.date }}</td>
                      <td class="num">{{ r.total }}</td>
                      <td class="num">{{ r.top }}</td>
                      <td class="num">{{ r.second }}</td>
                      <td class="num">{{ r.other }}</td>
                    </tr>
                  </tbody>
                </table>
              </div>
              <EChart v-else :option="dayOption" :label="t('timeline.chart')" :height="220" />
              <DateBrush
                v-model="range"
                :values="totals"
                :start-label="view.from"
                :end-label="view.to"
              />
              <div class="row" style="justify-content: space-between; margin-left: 32px">
                <div class="row" style="gap: 6px">
                  <button
                    v-for="q in quick"
                    :key="q.key"
                    class="btn small quick"
                    :class="{ on: q.on }"
                    :data-test="`quick-${q.key}`"
                    @click="pickQuick(q.len)"
                  >
                    {{ q.label }}
                  </button>
                </div>
                <div class="muted num" style="font-size: 12px" data-test="selected-range">
                  {{
                    t('timeline.selected', {
                      from: view.from,
                      to: view.to,
                      days: view.idx.length,
                      first: dateOf(0),
                      last: dateOf(days - 1),
                    })
                  }}
                </div>
              </div>
              <div class="muted" style="font-size: 12px">{{ t('timeline.hint') }}</div>
            </template>
          </template>
          <template v-else>
            <div class="muted">{{ t('timeline.monthHint', { name: topName }) }}</div>
            <EChart :option="monthOption" :label="t('timeline.byMonth')" :height="240" />
          </template>
        </div>

        <div class="row" style="justify-content: space-between">
          <div class="muted">{{ t('timeline.periods', { pool: poolName(game, selected) }) }}</div>
          <label class="check">
            <input v-model="showSkipped" type="checkbox" data-test="show-skipped" />
            {{ t('timeline.showSkipped') }}
          </label>
        </div>
        <div class="periods" data-test="periods">
          <template v-for="p in visiblePeriods" :key="p.key">
            <div v-if="p.joined" class="card period" data-test="period">
              <div class="row" style="justify-content: space-between; flex-wrap: nowrap; align-items: flex-start">
                <div class="ptitle">
                  <span v-if="p.title" class="pname">{{ p.title }}</span>
                  <span v-for="u in p.upTop" :key="u" class="r-top">{{ u }}</span>
                  <span v-for="u in p.upSecond" :key="u" class="r-second small">{{ u }}</span>
                </div>
                <span class="muted num" style="white-space: nowrap">{{ p.dates }}</span>
              </div>
              <div class="gotrow">
                <span>{{ t('timeline.periodPulls', { n: fmtInt(p.period.pulls, locale) }) }}</span>
                <span class="muted">·</span>
                <template v-if="p.period.tops.length">
                  <span v-for="(g, i) in p.period.tops" :key="i" class="got">
                    <ItemIcon :game="game" :name="g.itemName" :size="20" />
                    <span class="r-top">{{ g.itemName }}</span>
                    <span class="num muted">{{ t('common.pulls', { n: g.pulls }) }}</span>
                    <span v-if="hasUp && g.up" :class="markClass(g.up)">{{ markText(g.up) }}</span>
                  </span>
                </template>
                <span v-else class="muted">{{ t('timeline.noTop', { name: topName }) }}</span>
              </div>
            </div>
            <div v-else class="skipped" data-test="period-skipped">
              <span>
                <template v-if="p.title">{{ p.title }} </template>{{ [...p.upTop].join(' / ') }} ·
                {{ t('timeline.skipped') }}
              </span>
              <span class="num">{{ p.dates }}</span>
            </div>
          </template>
        </div>
        <div v-if="hasAuto" class="muted" style="font-size: 12px">{{ t('timeline.autoNote') }}</div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.legend {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--text2);
}
.sw {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 2px;
  margin-right: 4px;
  vertical-align: -1px;
}
.quick.on {
  border-color: var(--primary);
  color: var(--primary);
  background: var(--selected);
}
.periods {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.period {
  gap: 8px;
}
.ptitle {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
  align-items: baseline;
  font-weight: 600;
}
.ptitle .small {
  font-size: 12px;
  font-weight: 400;
}
.pname {
  color: var(--text);
}
.gotrow {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 10px;
  font-size: 13px;
}
.got {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.skipped {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 8px 14px;
  border: 1px dashed var(--border);
  border-radius: 8px;
  color: var(--text2);
  font-size: 13px;
}
</style>
