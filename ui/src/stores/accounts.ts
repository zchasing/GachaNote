import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { AccountDto, AccountStatsDto, OverviewEntry } from '@/bindings'
import { api } from '@/api'

/**
 * 账号与统计。前端不做统计计算，只缓存后端结果；收到 `data://changed` 时让受影响的缓存失效。
 */
export const useAccountsStore = defineStore('accounts', () => {
  const accounts = ref<AccountDto[]>([])
  const stats = ref<Record<string, AccountStatsDto>>({})
  const overview = ref<OverviewEntry[] | null>(null)
  /** 数据版本：任何写入后加 1，记录页据此重新查询。 */
  const version = ref(0)

  async function load() {
    accounts.value = await api.listAccounts()
  }

  function account(id: string): AccountDto | undefined {
    return accounts.value.find((a) => a.id === id)
  }

  function ofGame(game: string): AccountDto[] {
    return accounts.value.filter((a) => a.game === game)
  }

  const games = computed(() => new Set(accounts.value.map((a) => a.game)))

  async function loadStats(id: string, force = false): Promise<AccountStatsDto> {
    const cached = stats.value[id]
    if (cached && !force) return cached
    const s = await api.getStats(id)
    stats.value = { ...stats.value, [id]: s }
    return s
  }

  async function loadOverview(): Promise<OverviewEntry[]> {
    const entries = await api.getOverview()
    overview.value = entries
    const next = { ...stats.value }
    for (const e of entries) next[e.account.id] = e.stats
    stats.value = next
    return entries
  }

  /** 数据有变化：`ids` 为空表示全部。 */
  async function invalidate(ids: string[]) {
    if (ids.length === 0) {
      stats.value = {}
    } else {
      const next = { ...stats.value }
      for (const id of ids) delete next[id]
      stats.value = next
    }
    version.value += 1
    await load()
    if (overview.value) await loadOverview()
  }

  return {
    accounts,
    stats,
    overview,
    version,
    games,
    load,
    account,
    ofGame,
    loadStats,
    loadOverview,
    invalidate,
  }
})
