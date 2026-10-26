<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '@/components/AppIcon.vue'
import OverviewCard from '@/components/OverviewCard.vue'
import PageHeader from '@/components/PageHeader.vue'
import { fmtInt } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useUiStore } from '@/stores/ui'

// 跨游戏总览（UI/UX 文档 3.2）：每个账号一张卡片，顶栏可按游戏筛选。
const router = useRouter()
const accounts = useAccountsStore()
const games = useGamesStore()
const ui = useUiStore()
const { report } = useErrors()
const { t, locale, gameName } = useNames()

const loading = ref(true)
const filter = ref<string>('all')

async function load() {
  try {
    await accounts.loadOverview()
  } catch (e) {
    report(e)
  } finally {
    loading.value = false
  }
}
onMounted(load)
// 导入或删除后账号列表会变化，总览跟着刷新
watch(
  () => accounts.version,
  () => void load(),
)

const entries = computed(() => accounts.overview ?? [])
const gamesWithAccounts = computed(() => games.games.filter((g) => accounts.games.has(g.id)))
const shown = computed(() =>
  filter.value === 'all' ? entries.value : entries.value.filter((e) => e.account.game === filter.value),
)
const totalPulls = computed(() => entries.value.reduce((s, e) => s + e.stats.totalPulls, 0))
</script>

<template>
  <PageHeader
    :title="t('overview.title')"
    :subtitle="
      entries.length
        ? t('overview.summary', { accounts: entries.length, pulls: fmtInt(totalPulls, locale) })
        : t('overview.summaryEmpty')
    "
  >
    <template #icon><AppIcon name="grid" :size="24" /></template>
    <template v-if="gamesWithAccounts.length > 1" #tabs>
      <div class="tabs" role="tablist">
        <button class="tab" :class="{ on: filter === 'all' }" role="tab" @click="filter = 'all'">
          {{ t('overview.filterAll') }}
        </button>
        <button
          v-for="g in gamesWithAccounts"
          :key="g.id"
          class="tab"
          :class="{ on: filter === g.id }"
          role="tab"
          :data-test="`filter-${g.id}`"
          @click="filter = g.id"
        >
          <span class="dot" :style="{ background: g.themeColor }" />{{ gameName(g.id) }}
        </button>
      </div>
    </template>
  </PageHeader>
  <div class="content">
    <div class="wrap">
      <div v-if="loading && !accounts.overview" class="grid">
        <div v-for="i in 3" :key="i" class="skeleton" style="height: 260px" />
      </div>
      <div v-else-if="entries.length === 0" class="empty" data-test="overview-empty">
        <div>{{ t('overview.emptyTitle') }}</div>
        <div class="row" style="justify-content: center">
          <button class="btn primary" @click="ui.openImport({ method: 'url' })">
            <AppIcon name="link" />{{ t('overview.fromGame') }}
          </button>
          <button class="btn" @click="ui.openImport({ method: 'file' })">
            <AppIcon name="file" />{{ t('overview.importFile') }}
          </button>
        </div>
      </div>
      <div v-else class="grid">
        <OverviewCard
          v-for="e in shown"
          :key="e.account.id"
          :account="e.account"
          :stats="e.stats"
          @open="router.push(`/account/${e.account.id}/stats`)"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
  gap: 16px;
}
</style>
