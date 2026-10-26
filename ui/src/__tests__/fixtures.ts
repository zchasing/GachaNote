// 前端测试用的后端数据与 IPC 模拟。数字按后端 get_stats 的结果手写，界面只负责展示。
import { mockIPC } from '@tauri-apps/api/mocks'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { createMemoryHistory } from 'vue-router'
import type { AccountDto, AccountStatsDto, GameDto, PoolStatsDto, RecordPage, Settings } from '@/bindings'
import App from '@/App.vue'
import { i18n, setLocale } from '@/i18n'
import { createAppRouter } from '@/router'

const pool = (id: string, extra: Partial<GameDto['pools'][number]> = {}) => ({
  id,
  rarityTop: 5,
  hard: 90,
  up: 'none' as const,
  upRate: null,
  guarantee: false,
  spark: null,
  upWithin: null,
  pathMax: null,
  perBanner: false,
  expectedPulls: 62.3,
  ...extra,
})

export const GAMES: GameDto[] = [
  {
    id: 'genshin',
    vendor: 'hoyoverse',
    themeColor: '#3b6fd4',
    pools: [
      pool('character', { up: 'fiftyFifty', upRate: 0.5, guarantee: true }),
      pool('weapon', { hard: 80, up: 'fixed', pathMax: 1, expectedPulls: 53.8 }),
      pool('standard'),
    ],
    fetch: 'url',
    canFetch: true,
    formats: ['uigf_v4', 'gnj'],
    rarityTop: 5,
  },
  {
    id: 'starrail',
    vendor: 'hoyoverse',
    themeColor: '#7b5cd6',
    pools: [pool('character', { up: 'fiftyFifty', guarantee: true }), pool('standard')],
    fetch: 'url',
    canFetch: true,
    formats: ['uigf_v4'],
    rarityTop: 5,
  },
  {
    id: 'arknights',
    vendor: 'hypergryph',
    themeColor: '#1f2937',
    pools: [pool('standard', { rarityTop: 6, hard: 99, spark: 300 })],
    fetch: null,
    canFetch: false,
    formats: ['ak_export_csv', 'gnj'],
    rarityTop: 6,
  },
]

export const ACCOUNT: AccountDto = {
  id: 'acc-1',
  game: 'genshin',
  uid: '100000001',
  region: 'cn_gf01',
  nickname: null,
  tzOffsetMin: 480,
  createdAt: Date.UTC(2026, 0, 1),
  records: 52,
  lastImportAt: Date.now() - 3 * 3_600_000,
}

const emptyPity = {
  sinceTop: 0,
  remainingToHard: 90,
  guaranteed: false,
  guaranteeUncertain: false,
  pathPoints: null,
  sinceUp: null,
  bannerPulls: 0,
}

const charPool: PoolStatsDto = {
  poolType: 'character',
  totalPulls: 40,
  freeCount: 0,
  rarityCounts: [
    { rarity: 5, count: 2 },
    { rarity: 4, count: 5 },
    { rarity: 3, count: 33 },
  ],
  tops: [
    {
      itemName: '刻晴',
      pulledAt: Date.UTC(2026, 1, 1),
      pulls: 10,
      up: 'notUp',
      estimated: false,
      wasGuaranteed: false,
    },
    {
      itemName: '纳西妲',
      pulledAt: Date.UTC(2026, 2, 1),
      pulls: 77,
      up: 'up',
      estimated: true,
      wasGuaranteed: true,
    },
  ],
  avgPullsPerTop: 43.5,
  upCount: 1,
  lostCount: 1,
  avgPullsPerUp: 87,
  loss: { lost: 1, total: 1, rate: 1 },
  unknownCount: 0,
  estimatedCount: 1,
  pity: { ...emptyPity, sinceTop: 3, remainingToHard: 87 },
  firstAt: Date.UTC(2026, 0, 10),
  lastAt: Date.UTC(2026, 2, 5),
}

const stdPool: PoolStatsDto = {
  ...charPool,
  poolType: 'standard',
  totalPulls: 12,
  rarityCounts: [
    { rarity: 4, count: 1 },
    { rarity: 3, count: 11 },
  ],
  tops: [],
  avgPullsPerTop: null,
  upCount: 0,
  lostCount: 0,
  avgPullsPerUp: null,
  loss: null,
  estimatedCount: 0,
  pity: { ...emptyPity, sinceTop: 12, remainingToHard: 78 },
}

export const STATS: AccountStatsDto = {
  accountId: ACCOUNT.id,
  pools: [
    charPool,
    { ...stdPool, poolType: 'weapon', totalPulls: 0, rarityCounts: [], pity: emptyPity },
    stdPool,
  ],
  totalPulls: 52,
  rarityCounts: [
    { rarity: 5, count: 2 },
    { rarity: 4, count: 6 },
    { rarity: 3, count: 44 },
  ],
  topCount: 2,
  avgPullsPerTop: 43.5,
  upCount: 1,
  lostCount: 1,
  standardTopCount: 0,
  avgPullsPerUp: 87,
  loss: { lost: 1, total: 1, rate: 1 },
  hasEstimate: true,
  unrated: 0,
  firstAt: Date.UTC(2026, 0, 10),
  lastAt: Date.UTC(2026, 2, 5),
}

export const PAGE: RecordPage = {
  items: [
    {
      id: 'r2',
      poolType: 'character',
      itemName: '纳西妲',
      itemType: '角色',
      rarity: 5,
      pulledAt: Date.UTC(2026, 2, 1, 4, 5),
      isPull: true,
      pulls: 77,
      up: 'up',
      estimated: true,
    },
    {
      id: 'r1',
      poolType: 'character',
      itemName: '香菱',
      itemType: '角色',
      rarity: 4,
      pulledAt: Date.UTC(2026, 1, 20),
      isPull: true,
      pulls: 6,
      up: null,
      estimated: false,
    },
  ],
  total: 2,
  page: 0,
  pageSize: 50,
}

export const SETTINGS: Settings = {
  language: 'zh-CN',
  theme: 'light',
  proxy: null,
  sidebarGames: [],
  lastAccounts: {},
  metaAutoUpdate: false,
}

export type Handler = (args: Record<string, unknown>) => unknown

/** 默认的后端响应；测试可以按命令名覆盖。记录每次调用便于断言。 */
export function setupIpc(overrides: Record<string, Handler> = {}) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = []
  let accounts = [ACCOUNT]
  const defaults: Record<string, Handler> = {
    list_games: () => GAMES,
    list_accounts: () => accounts,
    get_settings: () => SETTINGS,
    set_settings: () => null,
    get_stats: () => STATS,
    get_overview: () => accounts.map((account) => ({ account, stats: STATS })),
    query_records: () => PAGE,
    app_info: () => ({
      version: '0.1.0',
      dataDir: '/data/GachaNote',
      defaultDataDir: '/data/GachaNote',
      logDir: '/data/GachaNote/logs',
      dbSize: 4096,
      canDiscover: false,
    }),
    list_backups: () => [],
    meta_info: () => ({ version: '2026.10.1', origin: 'bundled' }),
  }
  const handlers = { ...defaults, ...overrides }
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>
      calls.push({ cmd, args: a })
      const h = handlers[cmd]
      return h ? h(a) : null
    },
    { shouldMockEvents: true },
  )
  return {
    calls,
    called: (cmd: string) => calls.filter((c) => c.cmd === cmd),
    setAccounts: (a: AccountDto[]) => (accounts = a),
  }
}

/** 挂载整个应用并跳到指定路由。 */
export async function mountApp(path = '/') {
  const pinia = createPinia()
  const router = createAppRouter(createMemoryHistory())
  setLocale('zh-CN')
  await router.push(path)
  const wrapper = mount(App, { global: { plugins: [pinia, i18n, router] }, attachTo: document.body })
  await router.isReady()
  await settle()
  return { wrapper, router, pinia }
}

/** 等待异步命令与懒加载路由。 */
export async function settle(rounds = 6) {
  for (let i = 0; i < rounds; i++) {
    await flushPromises()
    await new Promise((r) => setTimeout(r, 0))
  }
}
