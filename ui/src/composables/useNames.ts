// 游戏、卡池、星级的显示名称：来自语言包，缺失时回退到标识符本身。
import { useI18n } from 'vue-i18n'

export function rarityClass(rarity: number, top: number): string {
  if (rarity >= top) return 'r-top'
  if (rarity === top - 1) return 'r-second'
  if (rarity === top - 2) return 'r-third'
  return 'r-low'
}

export function useNames() {
  const { t, te, locale } = useI18n()

  const gameName = (game: string) => (te(`games.${game}.name`) ? t(`games.${game}.name`) : game)
  const gameMono = (game: string) =>
    te(`games.${game}.mono`) ? t(`games.${game}.mono`) : game.slice(0, 1).toUpperCase()
  const poolName = (game: string, pool: string) =>
    te(`pools.${game}.${pool}.name`) ? t(`pools.${game}.${pool}.name`) : pool
  const poolShort = (game: string, pool: string) =>
    te(`pools.${game}.${pool}.short`) ? t(`pools.${game}.${pool}.short`) : pool
  /** 星级叫法跟随游戏：绝区零为 S / A / B 级，其余为 N 星。 */
  const rarityName = (game: string, rarity: number) =>
    game === 'zzz' && te(`rarity.zzz.${rarity}`) ? t(`rarity.zzz.${rarity}`) : t('rarity.star', { n: rarity })

  return { t, locale, gameName, gameMono, poolName, poolShort, rarityName }
}
