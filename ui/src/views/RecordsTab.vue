<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import type { RecordDto, RecordPage } from '@/bindings'
import { api } from '@/api'
import AppPager from '@/components/AppPager.vue'
import DropdownSelect from '@/components/DropdownSelect.vue'
import { fmtDateTime } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { rarityClass, useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useUiStore } from '@/stores/ui'

// 记录标签（UI/UX 文档 3.4）：筛选与分页都在后端 SQL 里做，当次抽数与 UP 标记来自后端缓存。
const route = useRoute()
const accounts = useAccountsStore()
const games = useGamesStore()
const ui = useUiStore()
const { report } = useErrors()
const { t, poolName, rarityName } = useNames()

const id = computed(() => String(route.params.id))
const account = computed(() => accounts.account(id.value))
const game = computed(() => account.value?.game ?? '')
const info = computed(() => games.game(game.value))
const rarityTop = computed(() => info.value?.rarityTop ?? 5)
const tz = computed(() => account.value?.tzOffsetMin ?? 480)

type Range = 'all' | 'd30' | 'd90' | 'd365'
const DAYS: Record<Range, number | null> = { all: null, d30: 30, d90: 90, d365: 365 }

/** 卡池筛选与统计标签共用 `ui.selectedPool`；统计页的「总览」对应这里的「全部」。 */
const pool = computed<string>({
  get: () => ui.selectedPool[id.value] ?? 'overview',
  set: (v) => (ui.selectedPool = { ...ui.selectedPool, [id.value]: v }),
})
const rarities = ref<number[]>([])
const range = ref<Range>('all')
const search = ref('')
const name = ref('')
const page = ref(0)
const size = ref(50)

const poolOptions = computed(() => [
  { value: 'overview', label: t('common.all') },
  ...(info.value?.pools ?? []).map((p) => ({ value: p.id, label: poolName(game.value, p.id) })),
])
const rarityOptions = computed(() => {
  const top = rarityTop.value
  return [top, top - 1, top - 2].map((r) => ({ value: r, label: rarityName(game.value, r) }))
})
const rangeOptions = computed(() =>
  (Object.keys(DAYS) as Range[]).map((r) => ({ value: r, label: t(`records.ranges.${r}`) })),
)

const onlyTop = computed({
  get: () => rarities.value.length === 1 && rarities.value[0] === rarityTop.value,
  set: (on: boolean) => (rarities.value = on ? [rarityTop.value] : []),
})

// 名称搜索防抖 250 ms
let timer: ReturnType<typeof setTimeout> | undefined
watch(search, (v) => {
  clearTimeout(timer)
  timer = setTimeout(() => (name.value = v.trim()), 250)
})
onBeforeUnmount(() => clearTimeout(timer))

const result = ref<RecordPage | null>(null)
const loading = ref(false)
let seq = 0

async function load() {
  const mine = ++seq
  loading.value = true
  const days = DAYS[range.value]
  try {
    const res = await api.queryRecords({
      accountId: id.value,
      poolTypes: pool.value === 'overview' ? [] : [pool.value],
      // 「其余星级」选项代表次次高星及以下
      rarities: rarities.value.flatMap((r) =>
        r === rarityTop.value - 2 ? Array.from({ length: r }, (_, i) => r - i) : [r],
      ),
      from: days === null ? null : Date.now() - days * 86_400_000,
      to: null,
      name: name.value || null,
      page: page.value,
      pageSize: size.value,
    })
    // 只保留最后一次查询的结果，避免快速切换筛选时旧结果覆盖新结果
    if (mine === seq) result.value = res
  } catch (e) {
    if (mine === seq) report(e)
  } finally {
    if (mine === seq) loading.value = false
  }
}

// 改筛选回到第一页
watch([pool, rarities, range, name, size], () => {
  if (page.value === 0) void load()
  else page.value = 0
})
watch(page, () => void load())
watch(
  [id, () => accounts.version],
  () => void load(),
  { immediate: true },
)

const items = computed(() => result.value?.items ?? [])

function markClass(r: RecordDto) {
  if (r.up === 'up') return 'tag up'
  if (r.up === 'notUp') return 'tag loss'
  return 'tag neutral'
}
function markText(r: RecordDto) {
  if (r.up === 'up') return t('stats.up')
  if (r.up === 'notUp') return t('stats.lost')
  return t('stats.unknown')
}
</script>

<template>
  <div class="content">
    <div class="wrap">
      <div class="filters">
        <label class="field">
          {{ t('records.pool') }}
          <DropdownSelect v-model="pool" :options="poolOptions" :label="t('records.pool')" />
        </label>
        <label class="field">
          {{ t('records.rarity') }}
          <DropdownSelect v-model="rarities" :options="rarityOptions" multiple :label="t('records.rarity')" />
        </label>
        <label class="field">
          {{ t('records.range') }}
          <DropdownSelect v-model="range" :options="rangeOptions" :label="t('records.range')" />
        </label>
        <label class="field" style="flex: 1 1 200px">
          {{ t('records.search') }}
          <input
            v-model="search"
            class="input"
            type="search"
            :placeholder="t('records.searchPlaceholder')"
            data-test="record-search"
          />
        </label>
        <label class="check" style="align-self: flex-end; min-height: 34px">
          <input v-model="onlyTop" type="checkbox" data-test="only-top" />
          {{ t('records.onlyTop', { name: rarityName(game, rarityTop) }) }}
        </label>
      </div>

      <div class="tablebox" :class="{ busy: loading }">
        <table data-test="records-table">
          <thead>
            <tr>
              <th>{{ t('records.columns.time') }}</th>
              <th>{{ t('records.columns.pool') }}</th>
              <th>{{ t('records.columns.item') }}</th>
              <th>{{ t('records.columns.type') }}</th>
              <th>{{ t('records.columns.rarity') }}</th>
              <th style="text-align: right">{{ t('records.columns.pulls') }}</th>
              <th>{{ t('records.columns.mark') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in items" :key="r.id" data-test="record-row">
              <td class="num muted">{{ fmtDateTime(r.pulledAt, tz) }}</td>
              <td>{{ poolName(game, r.poolType) }}</td>
              <td :class="rarityClass(r.rarity, rarityTop)">
                {{ r.itemName }}
                <span v-if="!r.isPull" class="tag neutral">{{ t('records.free') }}</span>
              </td>
              <td class="muted">{{ r.itemType ?? '—' }}</td>
              <td :class="rarityClass(r.rarity, rarityTop)">{{ rarityName(game, r.rarity) }}</td>
              <td class="num" style="text-align: right">{{ r.pulls ?? '' }}</td>
              <td>
                <template v-if="r.up">
                  <span :class="markClass(r)">{{ markText(r) }}</span>
                  <span v-if="r.estimated" class="tag est" style="margin-left: 4px">{{ t('common.estimated') }}</span>
                </template>
              </td>
            </tr>
            <tr v-if="result && items.length === 0">
              <td colspan="7" class="muted" style="text-align: center; padding: 32px">{{ t('records.empty') }}</td>
            </tr>
          </tbody>
        </table>
      </div>
      <AppPager v-if="result" v-model:page="page" v-model:size="size" :total="result.total" />
    </div>
  </div>
</template>

<style scoped>
.filters {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
  align-items: flex-end;
}
.tablebox {
  max-height: calc(100vh - 320px);
  min-height: 240px;
  transition: opacity var(--dur);
}
.tablebox.busy {
  opacity: 0.6;
}
</style>
