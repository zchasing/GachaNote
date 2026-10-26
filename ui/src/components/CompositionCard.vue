<script setup lang="ts">
import { computed } from 'vue'
import type { RarityCount } from '@/bindings'
import { fmtAvg, fmtInt, fmtPct } from '@/composables/format'
import { useNames } from '@/composables/useNames'

// 抽卡构成：环形图 + 三档星级数量与占比，最高星下方写 UP / 歪拆分（UI/UX 文档 3.3、3.5）。
const props = defineProps<{
  game: string
  rarityTop: number
  counts: RarityCount[]
  total: number
  upCount: number
  lostCount: number
  /** 账号总览时显示常驻池最高星数；为空表示不显示。 */
  standardCount?: number | null
  /** 是否显示 UP / 歪拆分（常驻池不显示）。 */
  showUp: boolean
  avgTop: number | null
  title: string
  subtitle?: string
}>()
const { t, locale, rarityName } = useNames()

/** 三档：最高星、次高星、其余合并。 */
const tiers = computed(() => {
  const top = props.rarityTop
  const get = (r: number) => props.counts.find((c) => c.rarity === r)?.count ?? 0
  const low = props.counts.filter((c) => c.rarity < top - 1).reduce((s, c) => s + c.count, 0)
  return [
    { key: 'top', label: rarityName(props.game, top), count: get(top), color: 'var(--bar5)' },
    { key: 'second', label: rarityName(props.game, top - 1), count: get(top - 1), color: 'var(--bar4)' },
    {
      key: 'low',
      label: t('rarity.andBelow', { name: rarityName(props.game, top - 2) }),
      count: low,
      color: 'var(--bar3)',
    },
  ]
})

const sum = computed(() => tiers.value.reduce((s, x) => s + x.count, 0))

/** 每段的 stroke-dasharray / offset（周长 100）。从低到高叠放，金色在最上。 */
const arcs = computed(() => {
  const total = sum.value || 1
  let offset = 0
  return [...tiers.value].reverse().map((tier) => {
    const len = (tier.count / total) * 100
    const arc = { ...tier, dash: `${len} ${100 - len}`, offset: 25 - offset }
    offset += len
    return arc
  })
})
</script>

<template>
  <div class="card" data-test="composition">
    <div class="row" style="justify-content: space-between">
      <div class="ct">{{ title }}</div>
      <div v-if="subtitle" class="muted">{{ subtitle }}</div>
    </div>
    <div class="comp">
      <div class="donut">
        <svg viewBox="0 0 42 42" width="128" height="128" role="img" :aria-label="title">
          <circle cx="21" cy="21" r="15.915" fill="none" stroke="var(--track)" stroke-width="6" />
          <circle
            v-for="a in arcs"
            :key="a.key"
            cx="21"
            cy="21"
            r="15.915"
            fill="none"
            :stroke="a.color"
            stroke-width="6"
            :stroke-dasharray="a.dash"
            :stroke-dashoffset="a.offset"
          />
        </svg>
        <div class="dcenter">
          <b class="num">{{ fmtInt(total, locale) }}</b><span>{{ t('stats.totalPulls') }}</span>
        </div>
      </div>
      <div class="clist">
        <template v-for="tier in tiers" :key="tier.key">
          <div class="crow">
            <span><span class="sw" :style="{ background: tier.color }" />{{ tier.label }}</span>
            <span class="num">{{ fmtInt(tier.count, locale) }}</span>
            <span class="num muted" style="text-align: right">{{
              sum ? fmtPct(tier.count / sum, locale) : '—'
            }}</span>
          </div>
          <div v-if="tier.key === 'top' && showUp" class="csub muted">
            {{
              standardCount !== null && standardCount !== undefined
                ? t('stats.upSplitStd', { up: upCount, lost: lostCount, std: standardCount })
                : t('stats.upSplit', { up: upCount, lost: lostCount })
            }}
          </div>
        </template>
        <div v-if="avgTop" class="muted" style="margin-top: 4px">
          {{ t('stats.avgEvery', { n: fmtAvg(avgTop, locale), name: rarityName(game, rarityTop) }) }}
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.comp {
  display: flex;
  gap: 24px;
  align-items: center;
  flex-wrap: wrap;
}
.donut {
  position: relative;
  width: 128px;
  height: 128px;
  flex: 0 0 128px;
}
.dcenter {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.dcenter b {
  font-size: 20px;
  line-height: 1.1;
}
.dcenter span {
  font-size: 12px;
  color: var(--text2);
}
.clist {
  flex: 1 1 180px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
.crow {
  display: grid;
  grid-template-columns: 1fr auto 56px;
  gap: 12px;
  align-items: center;
}
.csub {
  padding-left: 14px;
  margin-top: -4px;
}
.sw {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 2px;
  margin-right: 6px;
  vertical-align: -1px;
}
</style>
