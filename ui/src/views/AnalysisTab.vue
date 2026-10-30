<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import type { AccountStatsDto, AchievementDto, MonthDto, PoolInfo, PoolStatsDto, PredictionDto } from '@/bindings'
import { api } from '@/api'
import CompositionCard from '@/components/CompositionCard.vue'
import EChart from '@/components/EChart.vue'
import PityTag from '@/components/PityTag.vue'
import SegmentedControl from '@/components/SegmentedControl.vue'
import { baseOption, luckTier, type CssVar } from '@/composables/chart'
import { fmtAvg, fmtDate, fmtPct } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useUiStore } from '@/stores/ui'

// 分析标签（UI/UX 文档 3.5，F-14–F-17）：概率预测、欧非评价、出金分布、月度趋势、成就。
// 数字都来自后端（get_stats / predict / get_monthly_trend），前端只做展示。
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
const loading = ref(false)
watch(
  [id, () => accounts.stats[id.value]],
  async ([, s]) => {
    if (s || loading.value) return
    loading.value = true
    try {
      await accounts.loadStats(id.value)
    } catch (e) {
      report(e)
    } finally {
      loading.value = false
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

// 与统计标签共用所选卡池；统计标签选「总览」时这里显示第一个卡池
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

// ---------- 概率预测 ----------

const n = ref(10)
const prediction = ref<PredictionDto | null>(null)
let seq = 0
watch(
  [id, selected, n, () => accounts.version],
  async () => {
    const pool = selected.value
    const pulls = Math.min(1000, Math.max(1, Math.floor(Number(n.value) || 1)))
    if (!pool) return
    const mine = ++seq
    try {
      const p = await api.predict(id.value, pool, pulls)
      if (mine === seq) prediction.value = p
    } catch (e) {
      if (mine === seq) report(e)
    }
  },
  { immediate: true },
)

const within = computed(() => {
  const p = prediction.value
  if (!p || !p.points.length || p.poolType !== selected.value) return null
  const k = Math.min(Math.max(1, Math.floor(Number(n.value) || 1)), p.points.length - 1)
  return { k, ...p.points[k]! }
})

const curveTable = ref(false)
const curveRows = computed(() => {
  const p = prediction.value
  if (!p) return []
  const step = p.points.length > 200 ? 20 : 10
  return p.points.filter((x) => x.pulls > 0 && (x.pulls % step === 0 || x.pulls === p.points.length - 1))
})

function curveOption(css: CssVar) {
  const p = prediction.value
  const base = baseOption(css)
  if (!p) return {}
  const start = p.sinceTop
  const pct = (v: number) => Math.round(v * 1000) / 10
  const series = [
    {
      name: t('analysis.curveTop', { name: topName.value }),
      type: 'line',
      showSymbol: false,
      smooth: false,
      lineStyle: { width: 2, color: css('--bar5') },
      itemStyle: { color: css('--bar5') },
      data: p.points.map((x) => [start + x.pulls, pct(x.top ?? 0)]),
    },
  ] as Record<string, unknown>[]
  const up = p.points[0]?.up !== null && p.points[0]?.up !== undefined
  if (up) {
    series.push({
      name: t('analysis.curveUp'),
      type: 'line',
      showSymbol: false,
      lineStyle: { width: 2, color: css('--up') },
      itemStyle: { color: css('--up') },
      data: p.points.map((x) => [start + x.pulls, pct(x.up ?? 0)]),
    })
  }
  const q50 = up ? p.upQ50 : p.topQ50
  const q90 = up ? p.upQ90 : p.topQ90
  const marks: Record<string, unknown>[] = [
    { xAxis: start, label: { formatter: t('analysis.now') }, lineStyle: { color: css('--text2'), type: 'solid' } },
  ]
  if (q50 !== null) marks.push({ xAxis: start + q50, label: { formatter: '50%' } })
  if (q90 !== null) marks.push({ xAxis: start + q90, label: { formatter: '90%' } })
  series[0]!.markLine = {
    symbol: 'none',
    silent: true,
    label: { color: css('--text2'), fontSize: 11 },
    lineStyle: { color: css('--primary'), type: 'dashed' },
    data: marks,
  }
  return {
    ...base,
    legend: base.legend,
    tooltip: {
      ...base.tooltip,
      valueFormatter: (v: number) => `${v}%`,
    },
    xAxis: {
      type: 'value',
      min: 0,
      max: start + p.points.length - 1,
      name: t('analysis.pityAxis'),
      nameLocation: 'middle',
      nameGap: 24,
      nameTextStyle: { color: css('--text2'), fontSize: 11 },
      ...base.axis,
      splitLine: { show: false },
    },
    yAxis: { type: 'value', min: 0, max: 100, axisLabel: { ...base.axis.axisLabel, formatter: '{value}%' }, splitLine: base.axis.splitLine },
    grid: { ...base.grid, bottom: 24 },
    series,
  }
}

// ---------- 欧非评价 ----------

const luck = computed(() => {
  const l = current.value?.stats.luck
  if (!l) return null
  return {
    ...l,
    tier: l.percentile === null ? null : luckTier(l.percentile),
    metricLabel: l.metric === 'perUp' ? t('stats.avgUp') : t('analysis.avgTopName', { name: topName.value }),
  }
})

// ---------- 出金分布 ----------

const distTable = ref(false)
const tops = computed(() => current.value?.stats.tops ?? [])

function upColor(css: CssVar, up: string | null) {
  if (!hasUp.value || up === null) return css('--bar5')
  if (up === 'up') return css('--up')
  if (up === 'notUp') return css('--loss')
  return css('--gray')
}

function distOption(css: CssVar) {
  const base = baseOption(css)
  const expected = current.value?.info?.expectedPulls ?? null
  return {
    ...base,
    legend: { show: false },
    tooltip: {
      ...base.tooltip,
      formatter: (items: { dataIndex: number }[]) => {
        const top = tops.value[items[0]?.dataIndex ?? 0]
        if (!top) return ''
        const mark = hasUp.value && top.up ? ` · ${upLabel(top.up)}` : ''
        return `${top.itemName}<br/>${fmtDate(top.pulledAt, tz.value)}<br/>${t('common.pulls', { n: top.pulls })}${mark}`
      },
    },
    xAxis: { type: 'category', data: tops.value.map((_, i) => String(i + 1)), ...base.axis, splitLine: { show: false } },
    yAxis: { type: 'value', ...base.axis },
    series: [
      {
        type: 'bar',
        barMaxWidth: 18,
        data: tops.value.map((top) => ({ value: top.pulls, itemStyle: { color: upColor(css, top.up), borderRadius: [3, 3, 0, 0] } })),
        markLine: expected
          ? {
              symbol: 'none',
              silent: true,
              lineStyle: { color: css('--text2'), type: 'dashed' },
              label: {
                color: css('--text2'),
                fontSize: 11,
                position: 'insideEndTop',
                formatter: t('analysis.expected', { n: fmtAvg(expected, locale.value) }),
              },
              data: [{ yAxis: expected }],
            }
          : undefined,
      },
    ],
  }
}

function upLabel(up: string) {
  return up === 'up' ? t('stats.up') : up === 'notUp' ? t('stats.lost') : t('stats.unknown')
}

// ---------- 月度趋势 ----------

const months = ref<MonthDto[]>([])
const monthTable = ref(false)
watch(
  [id, selected, () => accounts.version],
  async () => {
    if (!selected.value) return
    try {
      months.value = await api.getMonthlyTrend(id.value, selected.value)
    } catch (e) {
      report(e)
    }
  },
  { immediate: true },
)

function monthOption(css: CssVar) {
  const base = baseOption(css)
  return {
    ...base,
    xAxis: { type: 'category', data: months.value.map((m) => m.month), ...base.axis, splitLine: { show: false } },
    yAxis: [
      { type: 'value', ...base.axis },
      { type: 'value', ...base.axis, minInterval: 1, splitLine: { show: false } },
    ],
    series: [
      {
        name: t('analysis.monthPulls'),
        type: 'bar',
        barMaxWidth: 24,
        itemStyle: { color: css('--bar3'), borderRadius: [3, 3, 0, 0] },
        data: months.value.map((m) => m.pulls),
      },
      {
        name: t('analysis.monthTops', { name: topName.value }),
        type: 'line',
        yAxisIndex: 1,
        symbolSize: 6,
        lineStyle: { color: css('--bar5'), width: 2 },
        itemStyle: { color: css('--bar5') },
        data: months.value.map((m) => m.tops),
      },
    ],
  }
}

// ---------- 成就 ----------

const achievements = computed(() => current.value?.stats.achievements ?? [])

function achievementText(a: AchievementDto) {
  const name = topName.value
  const key = `analysis.ach.${a.kind}`
  return {
    title: t(`${key}.title`),
    rule: t(`${key}.rule`, { n: a.threshold, name }),
    value: t(`${key}.value`, { n: a.value, item: a.itemName ?? '', name }),
  }
}
</script>

<template>
  <div class="content">
    <div class="wrap">
      <div v-if="!stats" class="metrics">
        <div v-for="i in 2" :key="i" class="skeleton" style="height: 160px" />
      </div>
      <div v-else-if="withPulls.length === 0" class="empty">{{ t('stats.noData') }}</div>
      <template v-else-if="current">
        <SegmentedControl v-model="selected" :options="segments" :label="t('records.pool')" />

        <!-- 概率预测 -->
        <div class="card" data-test="prediction">
          <div class="row" style="justify-content: space-between">
            <div class="ct">{{ t('analysis.prediction') }}</div>
            <div class="row" style="gap: 8px">
              <PityTag
                v-if="current.info?.guarantee"
                :guaranteed="current.stats.pity.guaranteed"
                :uncertain="current.stats.pity.guaranteeUncertain"
              />
              <span class="muted">{{ t('analysis.sinceTop', { n: current.stats.pity.sinceTop }) }}</span>
            </div>
          </div>
          <div class="pred">
            <div class="big">
              <div>
                <div class="mlabel">{{ t('analysis.nextTop', { name: topName }) }}</div>
                <div class="mvalue num" data-test="next-top">{{ fmtPct(current.stats.nextTop, locale) }}</div>
              </div>
              <div v-if="current.stats.nextUp !== null">
                <div class="mlabel">{{ t('analysis.nextUp') }}</div>
                <div class="mvalue num" data-test="next-up">{{ fmtPct(current.stats.nextUp, locale) }}</div>
              </div>
              <div class="within">
                <label class="row" style="gap: 6px">
                  <span>{{ t('analysis.withinPrefix') }}</span>
                  <input
                    v-model.number="n"
                    class="input num"
                    type="number"
                    min="1"
                    max="1000"
                    style="width: 76px"
                    data-test="within-n"
                  />
                  <span>{{ t('analysis.withinSuffix') }}</span>
                </label>
                <div v-if="within" class="num" data-test="within">
                  {{ t('analysis.withinTop', { name: topName }) }} <b>{{ fmtPct(within.top, locale) }}</b>
                  <template v-if="within.up !== null">
                    · {{ t('analysis.withinUp') }} <b>{{ fmtPct(within.up, locale) }}</b>
                  </template>
                </div>
                <div v-if="prediction" class="muted">
                  {{ t('analysis.guaranteeTop', { n: prediction.topGuaranteedWithin, name: topName }) }}
                  <template v-if="prediction.upGuaranteedWithin !== null">
                    · {{ t('analysis.guaranteeUp', { n: prediction.upGuaranteedWithin }) }}
                  </template>
                </div>
              </div>
            </div>
            <div class="chartbox">
              <div class="row" style="justify-content: flex-end">
                <button class="linkbtn" data-test="toggle-curve" @click="curveTable = !curveTable">
                  {{ curveTable ? t('analysis.showChart') : t('analysis.showTable') }}
                </button>
              </div>
              <div v-if="curveTable" class="tablebox minitable">
                <table>
                  <thead>
                    <tr>
                      <th>{{ t('analysis.morePulls') }}</th>
                      <th>{{ t('analysis.withinTop', { name: topName }) }}</th>
                      <th v-if="hasUp">{{ t('analysis.withinUp') }}</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="r in curveRows" :key="r.pulls">
                      <td class="num">{{ r.pulls }}</td>
                      <td class="num">{{ fmtPct(r.top, locale) }}</td>
                      <td v-if="hasUp" class="num">{{ fmtPct(r.up, locale) }}</td>
                    </tr>
                  </tbody>
                </table>
              </div>
              <EChart v-else-if="prediction" :option="curveOption" :label="t('analysis.prediction')" :height="220" />
            </div>
          </div>
        </div>

        <!-- 欧非评价 + 构成 -->
        <div class="split">
          <div v-if="luck" class="card" data-test="luck">
            <div class="ct">{{ t('analysis.luck') }}</div>
            <template v-if="luck.tier">
              <div class="row" style="gap: 12px; align-items: baseline">
                <span class="tier" :class="luck.tier" data-test="luck-tier">{{ t(`analysis.tiers.${luck.tier}`) }}</span>
                <span>{{ t('analysis.beat', { p: fmtPct(luck.percentile, locale) }) }}</span>
              </div>
              <div class="track"><div class="fill" :style="{ width: `${(luck.percentile ?? 0) * 100}%` }" /></div>
            </template>
            <div v-else class="muted" data-test="luck-few">{{ t('analysis.fewSamples', { n: 5, name: topName }) }}</div>
            <div class="cmp">
              <div>
                <span class="muted">{{ t('analysis.yourAvg', { metric: luck.metricLabel }) }}</span>
                <b class="num">{{ fmtAvg(luck.userAvg, locale) }}</b>
              </div>
              <div>
                <span class="muted">{{ t('analysis.expectedAvg') }}</span>
                <b class="num">{{ fmtAvg(luck.expectedAvg, locale) }}</b>
              </div>
              <div>
                <span class="muted">{{ t('analysis.samples') }}</span>
                <b class="num">{{ luck.samples }}</b>
              </div>
            </div>
          </div>
          <div v-else class="card">
            <div class="ct">{{ t('analysis.luck') }}</div>
            <div class="muted">{{ t('stats.historyEmpty', { name: topName }) }}</div>
          </div>
          <CompositionCard
            :game="game"
            :rarity-top="current.info?.rarityTop ?? rarityTop"
            :counts="current.stats.rarityCounts"
            :total="current.stats.totalPulls"
            :up-count="current.stats.upCount"
            :lost-count="current.stats.lostCount"
            :show-up="current.info?.up === 'fiftyFifty'"
            :avg-top="current.stats.avgPullsPerTop"
            :title="t('stats.composition')"
            :subtitle="poolName(game, current.stats.poolType)"
          />
        </div>

        <!-- 出金分布 -->
        <div v-if="tops.length" class="card" data-test="distribution">
          <div class="row" style="justify-content: space-between">
            <div class="ct">{{ t('analysis.distribution', { name: topName }) }}</div>
            <div class="row" style="gap: 12px">
              <span v-if="hasUp" class="legend muted">
                <span><i class="dot" style="background: var(--up)" />{{ t('stats.up') }}</span>
                <span><i class="dot" style="background: var(--loss)" />{{ t('stats.lost') }}</span>
              </span>
              <button class="linkbtn" data-test="toggle-dist" @click="distTable = !distTable">
                {{ distTable ? t('analysis.showChart') : t('analysis.showTable') }}
              </button>
            </div>
          </div>
          <div v-if="distTable" class="tablebox minitable">
            <table>
              <thead>
                <tr>
                  <th>#</th>
                  <th>{{ t('records.columns.item') }}</th>
                  <th>{{ t('records.columns.time') }}</th>
                  <th>{{ t('stats.pulls') }}</th>
                  <th v-if="hasUp">{{ t('records.columns.mark') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(top, i) in tops" :key="i">
                  <td class="num">{{ i + 1 }}</td>
                  <td>{{ top.itemName }}</td>
                  <td class="num">{{ fmtDate(top.pulledAt, tz) }}</td>
                  <td class="num">{{ top.pulls }}</td>
                  <td v-if="hasUp">{{ top.up ? upLabel(top.up) : '—' }}</td>
                </tr>
              </tbody>
            </table>
          </div>
          <EChart v-else :option="distOption" :label="t('analysis.distribution', { name: topName })" />
        </div>

        <!-- 月度趋势 -->
        <div v-if="months.length" class="card" data-test="monthly">
          <div class="row" style="justify-content: space-between">
            <div class="ct">{{ t('analysis.monthly') }}</div>
            <button class="linkbtn" data-test="toggle-month" @click="monthTable = !monthTable">
              {{ monthTable ? t('analysis.showChart') : t('analysis.showTable') }}
            </button>
          </div>
          <div v-if="monthTable" class="tablebox minitable">
            <table>
              <thead>
                <tr>
                  <th>{{ t('analysis.month') }}</th>
                  <th>{{ t('analysis.monthPulls') }}</th>
                  <th>{{ t('analysis.monthTops', { name: topName }) }}</th>
                  <th v-if="hasUp">{{ t('stats.up') }}</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="m in [...months].reverse()" :key="m.month">
                  <td class="num">{{ m.month }}</td>
                  <td class="num">{{ m.pulls }}</td>
                  <td class="num">{{ m.tops }}</td>
                  <td v-if="hasUp" class="num">{{ m.ups }}</td>
                </tr>
              </tbody>
            </table>
          </div>
          <EChart v-else :option="monthOption" :label="t('analysis.monthly')" />
        </div>

        <!-- 成就 -->
        <div v-if="achievements.length" class="card" data-test="achievements">
          <div class="ct">{{ t('analysis.achievements') }}</div>
          <div class="badges">
            <div
              v-for="a in achievements"
              :key="a.kind"
              class="badge"
              :class="{ done: a.done }"
              :data-test="`ach-${a.kind}`"
            >
              <div class="row" style="justify-content: space-between">
                <b>{{ achievementText(a).title }}</b>
                <span v-if="a.done" class="tag est">{{ t('analysis.achieved') }}</span>
              </div>
              <div class="muted">{{ achievementText(a).rule }}</div>
              <div>{{ achievementText(a).value }}</div>
            </div>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.metrics {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}
.mlabel {
  color: var(--text2);
  font-size: 13px;
}
.mvalue {
  font-size: 30px;
  font-weight: 600;
  line-height: 1.15;
}
.pred {
  display: grid;
  grid-template-columns: minmax(240px, 1fr) minmax(0, 2fr);
  gap: 24px;
  align-items: start;
}
.big {
  display: flex;
  flex-wrap: wrap;
  gap: 16px 32px;
}
.within {
  flex-basis: 100%;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
.within b {
  font-weight: 600;
}
.chartbox {
  min-width: 0;
}
.minitable {
  max-height: 260px;
}
.split {
  display: grid;
  grid-template-columns: minmax(0, 3fr) minmax(300px, 2fr);
  gap: 16px;
  align-items: start;
}
.tier {
  font-size: 26px;
  font-weight: 700;
}
.tier.great,
.tier.lucky {
  color: var(--up);
}
.tier.even {
  color: var(--text);
}
.tier.unlucky,
.tier.awful {
  color: var(--loss);
}
.cmp {
  display: flex;
  gap: 0;
  flex-wrap: wrap;
  border-top: 1px solid var(--border);
  padding-top: 12px;
}
.cmp > div {
  flex: 1 1 120px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.cmp b {
  font-size: 16px;
  font-weight: 600;
}
.legend {
  display: flex;
  gap: 12px;
}
.legend .dot {
  margin-right: 6px;
}
.badges {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}
.badge {
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
  opacity: 0.55;
}
.badge.done {
  opacity: 1;
  background: var(--goldbg);
  border-color: var(--goldline);
}
@media (max-width: 1100px) {
  .split,
  .pred {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
