import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { GameDto, PoolInfo } from '@/bindings'
import { api } from '@/api'

/** 游戏信息（主题色、卡池规则、导入方式），启动时加载一次。 */
export const useGamesStore = defineStore('games', () => {
  const games = ref<GameDto[]>([])

  async function load() {
    games.value = await api.listGames()
  }

  const byId = computed(() => new Map(games.value.map((g) => [g.id, g])))

  function game(id: string): GameDto | undefined {
    return byId.value.get(id)
  }

  function pool(gameId: string, poolId: string): PoolInfo | undefined {
    return game(gameId)?.pools.find((p) => p.id === poolId)
  }

  return { games, load, game, pool }
})
