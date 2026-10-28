<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import type { AccountStatsDto, PoolInfo, PoolStatsDto, TopPullDto } from '@/bindings'
import CompositionCard from '@/components/CompositionCard.vue'
import PityTag from '@/components/PityTag.vue'
import SegmentedControl from '@/components/SegmentedControl.vue'
import { luckTier } from '@/composables/chart'
import { fmtAvg, fmtDate, fmtInt, fmtPct } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useUiStore } from '@/stores/ui'

// 统计标签（UI/UX 文档 3.3）：所有数字来自后端 get_stats，前端只做展示与格式化。
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

const stats = computed<AccountStatsDto | undefined>(() => accounts.stats[id.value])
const loading = ref(false)

async function load() {
  loading.value = true
  try {
    await accounts.loadStats(id.value)
  } catch (e) {
    report(e)
  } finally {
    loading.value = false
  }
}
// 数据变化时 store 会清掉缓存，这里跟着重新加载
watch(
  [id, () => accounts.stats[id.value]],
  ([, s]) => {
    if (!s && !loading.value) void load()
  },
  { immediate: true },
)

interface Row {
  stats: PoolStatsDto
  info: PoolInfo | undefined
}
const rows = computed<Row[]>(() =>
  (stats.value?.pools ?? []).map((p) => ({ stats: p, info: games.pool(game.value, p.poolType) })),
)
/** 卡池按钮只列有记录的卡池，没有记录的卡池不占位置。 */
const withPulls = computed(() => rows.value.filter((r) => r.stats.totalPulls > 0))

const selected = computed({
  get: () => {
    const v = ui.selectedPool[id.value] ?? 'overview'
    return v === 'overview' || withPulls.value.some((r) => r.stats.poolType === v) ? v : 'overview'
  },
  set: (v: string) => (ui.selectedPool = { ...ui.selectedPool, [id.value]: v }),
})

const segments = computed(() => [
  { value: 'overview', label: t('stats.overview') },
  ...withPulls.value.map((r) => ({
    value: r.stats.poolType,
    label: poolShort(game.value, r.stats.poolType),
    badge: r.info && r.info.hard > 0 ? r.stats.pity.sinceTop : null,
  })),
])

const current = computed(() => withPulls.value.find((r) => r.stats.poolType === selected.value))

function pctOf(n: number, of: number | null | undefined) {
  return `${Math.min(100, (n / Math.max(1, of ?? 1)) * 100)}%`
}

/** 单卡池的三张指标卡，按卡池规则选择（UI/UX 文档 3.3 表格）。 */
const cards = computed(() => {
  const r = current.value
  if (!r) return null
  const p = r.stats
  const info = r.info
  const hasUp = info?.up && info.up !== 'none'
  const second = hasUp
    ? {
        key: 'avgUp',
        label: t('stats.avgUp'),
        value: fmtAvg(p.avgPullsPerUp, locale.value),
        sub: t('stats.upCount', { n: p.upCount }),
        estimated: p.estimatedCount > 0,
      }
    : {
        key: 'avgTop',
        label: t('stats.avgTop'),
        value: fmtAvg(p.avgPullsPerTop, locale.value),
        sub: t('stats.topCountSub', { n: p.tops.length }),
        estimated: false,
      }
  let third: { key: string; label: string; value: string; sub: string; bar?: string; estimated: boolean }
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
      value: fmtPct(p.loss?.rate, locale.value),
      sub: p.loss ? t('stats.lossFrac', { lost: p.loss.lost, total: p.loss.total }) : '—',
      estimated: p.estimatedCount > 0,
    }
  }
  return { second, third, lossReplaced: third.key !== 'loss' && !!p.loss }
})

/** 出金历史：新的在上。颜色按与期望抽数的比值区分欧 / 平 / 非。 */
const history = computed(() => {
  const r = current.value
  if (!r) return []
  const expected = r.info?.expectedPulls ?? null
  const hard = r.info?.hard || Math.max(...r.stats.tops.map((x) => x.pulls), 1)
  return [...r.stats.tops].reverse().map((top) => ({
    top,
    width: pctOf(top.pulls, hard),
    tone: tone(top, expected),
  }))
})

function tone(top: TopPullDto, expected: number | null) {
  if (!expected) return 'even'
  const ratio = top.pulls / expected
  if (ratio < 0.85) return 'lucky'
  if (ratio > 1.15) return 'unlucky'
  return 'even'
}

function share(p: PoolStatsDto, rarity: number) {
  const c = p.rarityCounts.find((x) => x.rarity === rarity)?.count ?? 0
  return { count: c, rate: p.totalPulls ? c / p.totalPulls : null }
}

const tz = computed(() => account.value?.tzOffsetMin ?? 480)

/** 次级指标行末尾的一句话结论（v0.2）：下一抽出金概率 + 欧非评级，点击进入分析标签。 */
const verdict = computed(() => {
  const p = current.value?.stats
  if (!p) return null
  const parts = [t('stats.nextTop', { name: topName.value, p: fmtPct(p.nextTop, locale.value) })]
  const pct = p.luck?.percentile
  if (pct !== null && pct !== undefined) {
    parts.push(t('stats.luckShort', { tier: t(`analysis.tiers.${luckTier(pct)}`), p: fmtPct(1 - pct, locale.value) }))
  }
  return parts.join(' · ')
})
</script>

<template>
  <div class="content">
    <div class="wrap">
      <div v-if="!stats" class="metrics">
        <div v-for="i in 3" :key="i" class="skeleton" style="height: 128px" />
      </div>
      <template v-else-if="stats.totalPulls === 0">
        <div class="empty">{{ t('stats.noData') }}</div>
      </template>
      <template v-else>
        <SegmentedControl v-model="selected" :options="segments" :label="t('records.pool')" />

        <!-- 账号总览 -->
        <template v-if="selected === 'overview'">
          <div class="metrics" data-test="account-metrics">
            <div class="card metric">
              <div class="mlabel">{{ t('stats.totalPulls') }}</div>
              <div class="mvalue num">{{ fmtInt(stats.totalPulls, locale) }}</div>
              <div class="muted">{{ t('stats.topCount', { name: topName }) }} {{ stats.topCount }}</div>
            </div>
            <div class="card metric">
              <div class="mlabel">
                {{ t('stats.avgUp') }}
                <span v-if="stats.hasEstimate" class="tag est">{{ t('common.estimated') }}</span>
              </div>
              <div class="mvalue num">{{ fmtAvg(stats.avgPullsPerUp, locale) }}</div>
              <div class="muted">{{ t('stats.upCount', { n: stats.upCount }) }}</div>
            </div>
            <div class="card metric">
              <div class="mlabel">
                {{ t('stats.lossRate') }}
                <span v-if="stats.hasEstimate" class="tag est">{{ t('common.estimated') }}</span>
              </div>
              <div class="mvalue num" data-test="account-loss">{{ fmtPct(stats.loss?.rate, locale) }}</div>
              <div class="muted">
                {{ stats.loss ? t('stats.lossFrac', { lost: stats.loss.lost, total: stats.loss.total }) : '—' }}
              </div>
            </div>
          </div>
          <div class="split">
            <div class="card">
              <div class="ct">{{ t('stats.pools') }}</div>
              <button
                v-for="r in withPulls"
                :key="r.stats.poolType"
                class="poolrow"
                data-test="pool-row"
                @click="selected = r.stats.poolType"
              >
                <div class="row" style="gap: 8px">
                  <span>{{ poolName(game, r.stats.poolType) }}</span>
                  <PityTag
                    v-if="r.info?.guarantee"
                    :guaranteed="r.stats.pity.guaranteed"
                    :uncertain="r.stats.pity.guaranteeUncertain"
                  />
                  <span class="muted" style="margin-left: auto">
                    {{ t('common.pulls', { n: fmtInt(r.stats.totalPulls, locale) }) }}
                  </span>
                </div>
                <div v-if="r.info && r.info.hard > 0" class="row" style="gap: 10px; flex-wrap: nowrap">
                  <div class="track" style="flex: 1">
                    <div class="fill" :style="{ width: pctOf(r.stats.pity.sinceTop, r.info.hard) }" />
                  </div>
                  <span class="num muted">{{ r.stats.pity.sinceTop }} / {{ r.info.hard }}</span>
                </div>
              </button>
            </div>
            <CompositionCard
              :game="game"
              :rarity-top="rarityTop"
              :counts="stats.rarityCounts"
              :total="stats.totalPulls"
              :up-count="stats.upCount"
              :lost-count="stats.lostCount"
              :standard-count="stats.standardTopCount"
              :show-up="true"
              :avg-top="stats.avgPullsPerTop"
              :title="t('stats.compositionAll')"
            />
          </div>
          <div class="notes muted">
            <div v-if="stats.firstAt && stats.lastAt">
              {{ t('stats.range') }}：{{ fmtDate(stats.firstAt, tz) }} – {{ fmtDate(stats.lastAt, tz) }}
            </div>
            <div v-if="stats.hasEstimate">{{ t('stats.estimatedUp') }}</div>
            <div v-if="stats.unrated">{{ t('stats.unrated', { n: stats.unrated }) }}</div>
          </div>
        </template>

        <!-- 单卡池 -->
        <template v-else-if="current && cards">
          <div class="metrics" data-test="pool-metrics">
            <div class="card metric">
              <div class="mlabel">
                {{ t('stats.currentPity') }}
                <PityTag
                  v-if="current.info?.guarantee"
                  :guaranteed="current.stats.pity.guaranteed"
                  :uncertain="current.stats.pity.guaranteeUncertain"
                />
              </div>
              <div class="mvalue num" data-test="pity">
                {{ current.stats.pity.sinceTop }}
                <span class="muted" style="font-size: 14px">/ {{ current.info?.hard ?? '?' }}</span>
              </div>
              <div v-if="current.info && current.info.hard > 0" class="track">
                <div class="fill" :style="{ width: pctOf(current.stats.pity.sinceTop, current.info.hard) }" />
              </div>
              <div class="muted">
                {{ t('stats.remaining', { n: current.stats.pity.remainingToHard }) }}
                <template v-if="current.info?.pathMax && current.stats.pity.pathPoints !== null">
                  · {{ t('stats.path') }} {{ current.stats.pity.pathPoints }} / {{ current.info.pathMax }}
                </template>
              </div>
            </div>
            <div class="card metric">
              <div class="mlabel">
                {{ cards.second.label }}
                <span v-if="cards.second.estimated" class="tag est">{{ t('common.estimated') }}</span>
              </div>
              <div class="mvalue num">{{ cards.second.value }}</div>
              <div class="muted">{{ cards.second.sub }}</div>
            </div>
            <div class="card metric" :data-test="`metric-${cards.third.key}`">
              <div class="mlabel">
                {{ cards.third.label }}
                <span v-if="cards.third.estimated" class="tag est">{{ t('common.estimated') }}</span>
              </div>
              <div class="mvalue num">{{ cards.third.value }}</div>
              <div v-if="cards.third.bar" class="track">
                <div class="fill" :style="{ width: cards.third.bar }" />
              </div>
              <div class="muted">{{ cards.third.sub }}</div>
            </div>
          </div>

          <div class="secondary card">
            <div>
              <span class="muted">{{ t('stats.totalPulls') }}</span>
              <b class="num">{{ fmtInt(current.stats.totalPulls, locale) }}</b>
            </div>
            <div v-for="r in [rarityTop, rarityTop - 1]" :key="r">
              <span class="muted">{{ rarityName(game, r) }}</span>
              <b class="num">
                {{ share(current.stats, r).count }}
                <span class="muted">{{ fmtPct(share(current.stats, r).rate, locale) }}</span>
              </b>
            </div>
            <div>
              <span class="muted">{{ t('stats.avgTop') }}</span>
              <b class="num">{{ fmtAvg(current.stats.avgPullsPerTop, locale) }}</b>
            </div>
            <div v-if="cards.lossReplaced && current.stats.loss">
              <span class="muted">{{ t('stats.lossRate') }}</span>
              <b class="num">{{ fmtPct(current.stats.loss.rate, locale) }}</b>
            </div>
            <RouterLink v-if="verdict" :to="`/account/${id}/analysis`" class="verdict" data-test="verdict">
              <span class="muted">{{ t('stats.verdict') }}</span>
              <b class="num">{{ verdict }}</b>
            </RouterLink>
          </div>
          <div class="notes muted">
            <div v-if="current.stats.firstAt && current.stats.lastAt">
              {{ t('stats.range') }}：{{ fmtDate(current.stats.firstAt, tz) }} –
              {{ fmtDate(current.stats.lastAt, tz) }}
            </div>
            <div v-if="current.stats.freeCount">{{ t('stats.free', { n: current.stats.freeCount }) }}</div>
            <div v-if="current.stats.unknownCount">
              {{ t('stats.unknownUp', { n: current.stats.unknownCount, name: topName }) }}
            </div>
            <div v-if="current.stats.estimatedCount">{{ t('stats.estimatedUp') }}</div>
          </div>

          <div class="split">
            <div class="card">
              <div class="ct">{{ t('stats.history', { name: topName }) }}</div>
              <div v-if="history.length === 0" class="muted">{{ t('stats.historyEmpty', { name: topName }) }}</div>
              <div v-else class="hist" data-test="history">
                <div v-for="(h, i) in history" :key="i" class="hrow">
                  <span class="islot" />
                  <div class="hname">
                    <span class="r-top">{{ h.top.itemName }}</span>
                    <span class="muted">{{ fmtDate(h.top.pulledAt, tz) }}</span>
                  </div>
                  <div class="hbar">
                    <div class="track" style="height: 10px">
                      <div class="fill" :style="{ width: h.width, background: `var(--${h.tone})` }" />
                    </div>
                  </div>
                  <span class="num hpulls">{{ h.top.pulls }}</span>
                  <span class="hmark">
                    <template v-if="current.info?.up === 'fiftyFifty' || current.info?.up === 'fixed'">
                      <span v-if="h.top.up === 'up'" class="tag up">{{ t('stats.up') }}</span>
                      <span v-else-if="h.top.up === 'notUp'" class="tag loss">{{ t('stats.lost') }}</span>
                      <span v-else-if="h.top.up === 'unknown'" class="tag neutral">{{ t('stats.unknown') }}</span>
                      <span v-if="h.top.estimated" class="tag est">{{ t('common.estimated') }}</span>
                    </template>
                  </span>
                </div>
              </div>
              <div v-if="history.length && current.info?.expectedPulls" class="legend muted">
                <span><i class="dot" style="background: var(--lucky)" />{{ t('stats.legendLucky') }}</span>
                <span><i class="dot" style="background: var(--even)" />{{ t('stats.legendEven') }}</span>
                <span><i class="dot" style="background: var(--unlucky)" />{{ t('stats.legendUnlucky') }}</span>
              </div>
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
        </template>
      </template>
    </div>
  </div>
</template>

<style scoped>
.metrics {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 16px;
}
.metric {
  gap: 8px;
}
.mlabel {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text2);
  font-size: 13px;
}
.mvalue {
  font-size: 30px;
  font-weight: 600;
  line-height: 1.15;
}
.split {
  display: grid;
  grid-template-columns: minmax(0, 3fr) minmax(300px, 2fr);
  gap: 16px;
  align-items: start;
}
.poolrow {
  border: 1px solid var(--border);
  background: var(--bg);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  text-align: left;
  cursor: pointer;
  color: var(--text);
  font-size: 13px;
}
.poolrow:hover {
  border-color: var(--primary);
}
.secondary {
  flex-direction: row;
  flex-wrap: wrap;
  gap: 0;
  padding: 0;
}
.secondary > div,
.secondary > .verdict {
  flex: 1 1 140px;
  padding: 12px 16px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.secondary > * + * {
  border-left: 1px solid var(--border);
}
.verdict {
  color: inherit;
  text-decoration: none;
  flex-grow: 2 !important;
}
.verdict:hover b {
  color: var(--primary);
}
.secondary b {
  font-size: 16px;
  font-weight: 600;
}
.notes {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: -8px;
}
.hist {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 520px;
  overflow-y: auto;
}
.hrow {
  display: grid;
  grid-template-columns: 24px minmax(110px, 160px) 1fr 36px 84px;
  gap: 10px;
  align-items: center;
  font-size: 13px;
}
.hname {
  display: flex;
  flex-direction: column;
  line-height: 1.3;
  min-width: 0;
}
.hname > span:first-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.hpulls {
  text-align: right;
  font-weight: 600;
}
.hmark {
  display: flex;
  gap: 4px;
}
.legend {
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
}
.legend .dot {
  margin-right: 6px;
}
@media (max-width: 1100px) {
  .split {
    grid-template-columns: minmax(0, 1fr);
  }
}
</style>
