<script setup lang="ts">
import { computed, ref } from 'vue'
import type { AccountDto, AccountStatsDto, PoolStatsDto } from '@/bindings'
import { fmtAvg, fmtInt, fmtPct, maskUid } from '@/composables/format'
import { useNames } from '@/composables/useNames'
import { useRelative } from '@/composables/useRelative'
import { useGamesStore } from '@/stores/games'
import AppIcon from './AppIcon.vue'
import GameIcon from './GameIcon.vue'
import ItemIcon from './ItemIcon.vue'
import PityTag from './PityTag.vue'

// 总览账号卡片（UI/UX 文档 3.2）：默认显示角色与武器类限定池，其余卡池收在「展开」里。
const props = defineProps<{ account: AccountDto; stats: AccountStatsDto }>()
const emit = defineEmits<{ open: [] }>()
const games = useGamesStore()
const { t, locale, gameName, poolName, rarityName } = useNames()
const { rel, abs } = useRelative()
const expanded = ref(false)

const color = computed(() => games.game(props.account.game)?.themeColor ?? 'var(--primary)')

const rows = computed(() =>
  props.stats.pools.map((p) => ({ stats: p, info: games.pool(props.account.game, p.poolType) })),
)

/** 默认显示：有 UP 且有记录的卡池取前两个；都没有记录时取第一个有 UP 的卡池。 */
const featured = computed(() => {
  const up = rows.value.filter((r) => r.info && r.info.up !== 'none')
  const withPulls = up.filter((r) => r.stats.totalPulls > 0).slice(0, 2)
  return withPulls.length ? withPulls : up.slice(0, 1)
})
const rest = computed(() =>
  rows.value.filter((r) => !featured.value.includes(r) && r.stats.totalPulls > 0),
)
const visible = computed(() => (expanded.value ? [...featured.value, ...rest.value] : featured.value))

const title = computed(() => props.account.nickname ?? props.account.uid)

const topName = computed(() => rarityName(props.account.game, games.game(props.account.game)?.rarityTop ?? 5))

function pct(p: PoolStatsDto, hard: number) {
  return `${Math.min(100, (p.pity.sinceTop / Math.max(1, hard)) * 100)}%`
}
</script>

<template>
  <article class="card ocard" :style="{ '--game': color }" data-test="overview-card">
    <div class="stripe" />
    <div class="ohead">
      <GameIcon :game="account.game" :size="24" />
      <div style="min-width: 0; flex: 1">
        <div class="ct ellipsis">{{ gameName(account.game) }} · {{ title }}</div>
        <div class="muted" :title="abs(account.lastImportAt)">
          UID {{ maskUid(account.uid) }} · {{ t('overview.updated', { time: rel(account.lastImportAt) }) }}
        </div>
      </div>
      <button
        class="iconbtn"
        :aria-label="t('overview.openAccount', { name: title })"
        :title="t('stats.viewDetail')"
        data-test="open-account"
        @click="emit('open')"
      >
        <AppIcon name="chevronRight" :size="18" />
      </button>
    </div>
    <div class="opools">
      <div v-for="r in visible" :key="r.stats.poolType" class="opool" data-test="overview-pool">
        <div class="poolhead">
          <span>{{ poolName(account.game, r.stats.poolType) }}</span>
          <PityTag
            v-if="r.info?.guarantee"
            :guaranteed="r.stats.pity.guaranteed"
            :uncertain="r.stats.pity.guaranteeUncertain"
          />
          <span class="num" style="margin-left: auto">
            <b class="pity">{{ r.stats.pity.sinceTop }}</b>
            <span class="muted"> {{ t('pity.of', { n: r.info?.hard ?? '?' }) }}</span>
          </span>
        </div>
        <div class="track">
          <div class="fill" :style="{ width: pct(r.stats, r.info?.hard ?? 1), background: color }" />
        </div>
        <div v-if="r.stats.tops.length" class="olast muted" data-test="overview-last">
          <span>{{ t('overview.lastTop', { name: topName }) }}</span>
          <ItemIcon :game="account.game" :name="r.stats.tops.at(-1)!.itemName" :size="20" />
          <span class="r-top">{{ r.stats.tops.at(-1)!.itemName }}</span>
          <span class="num">{{ t('common.pulls', { n: r.stats.tops.at(-1)!.pulls }) }}</span>
        </div>
      </div>
      <button v-if="rest.length" class="linkbtn" style="align-self: flex-start" @click="expanded = !expanded">
        {{ expanded ? t('common.collapse') : t('common.expand') }}
      </button>
    </div>
    <div class="ostats">
      <div>
        <span class="muted">{{ t('overview.totalPulls') }}</span>
        <b class="num" data-test="overview-total">{{ fmtInt(stats.totalPulls, locale) }}</b>
      </div>
      <div>
        <span class="muted">{{ t('overview.avgUp') }}</span>
        <b class="num">{{ fmtAvg(stats.avgPullsPerUp, locale) }}</b>
      </div>
      <div>
        <span class="muted">{{ t('overview.lossRate') }}</span>
        <b class="num">{{ fmtPct(stats.loss?.rate, locale) }}</b>
      </div>
    </div>
  </article>
</template>

<style scoped>
.ocard {
  padding: 0;
  gap: 0;
  overflow: hidden;
}
.stripe {
  height: 3px;
  background: var(--game);
}
.ohead {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 12px 12px 16px;
}
.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.opools {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 0 16px 16px;
}
.opool {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.poolhead {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.pity {
  font-size: 18px;
}
.olast {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  min-width: 0;
}
.ostats {
  margin-top: auto;
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  border-top: 1px solid var(--border);
  background: color-mix(in srgb, var(--game) 9%, var(--surface));
}
.ostats > div {
  padding: 10px 16px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.ostats > div + div {
  border-left: 1px solid var(--border);
}
.ostats b {
  font-size: 15px;
  font-weight: 600;
}
</style>
