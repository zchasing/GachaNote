// 前端测试与 Playwright 冒烟测试共用的后端数据。数字按后端 get_stats 的结果手写，界面只负责展示。
import type {
  AccountDto,
  AccountStatsDto,
  GameDto,
  MonthDto,
  PoolStatsDto,
  PredictionDto,
  RecordPage,
  Settings,
  TimelineDto,
} from '@/bindings'

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
    fetch: 'account_token',
    canFetch: true,
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
  nextTop: 0.006,
  nextUp: 0.003,
  luck: { metric: 'perUp', samples: 1, userAvg: 87, expectedAvg: 93.5, percentile: null },
  achievements: [
    { kind: 'lucky', done: true, value: 10, threshold: 31, itemName: '刻晴' },
    { kind: 'unlucky', done: true, value: 77, threshold: 77, itemName: '纳西妲' },
    { kind: 'streak', done: false, value: 1, threshold: 3, itemName: null },
    { kind: 'longWait', done: false, value: 28, threshold: 45, itemName: null },
  ],
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
  nextUp: null,
  luck: null,
  achievements: [],
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
      itemId: '10000073',
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
      itemId: null,
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
  updateAutoCheck: false,
}

/** 原神角色池：已垫 3 抽，不是大保底。曲线按 0.6% 单抽、74 抽起软保底近似。 */
export function prediction(pulls = 10): PredictionDto {
  const points = []
  let miss = 1
  for (let k = 0; k <= Math.max(pulls, 87); k++) {
    if (k > 0) {
      const at = 3 + k
      const p = at >= 90 ? 1 : at >= 74 ? 0.006 + 0.06 * (at - 73) : 0.006
      miss *= 1 - Math.min(1, p)
    }
    const top = 1 - miss
    points.push({ pulls: k, top, up: top / 2 })
  }
  return {
    poolType: 'character',
    sinceTop: 3,
    guaranteed: false,
    points,
    topQ50: 72,
    topQ90: 78,
    upQ50: 85,
    upQ90: null,
    topGuaranteedWithin: 87,
    upGuaranteedWithin: 177,
  }
}

export const MONTHS: MonthDto[] = [
  { month: '2026-01', pulls: 10, tops: 0, ups: 0 },
  { month: '2026-02', pulls: 20, tops: 1, ups: 0 },
  { month: '2026-03', pulls: 10, tops: 1, ups: 1 },
]

/** 原神角色池时间线：2026-01-10 起 60 天，两期有记录、一期未参与、一段自动分段。 */
export function timeline(): TimelineDto {
  const n = 60
  const top = Array<number>(n).fill(0)
  const second = Array<number>(n).fill(0)
  const other = Array<number>(n).fill(0)
  other[0] = 10
  top[22] = 1
  second[22] = 2
  other[22] = 17
  top[50] = 1
  other[50] = 9
  return {
    poolType: 'character',
    daily: { start: '2026-01-10', top, second, other },
    periods: [
      {
        kind: 'banner',
        id: 'b3',
        names: {},
        rawName: null,
        start: Date.UTC(2026, 1, 25),
        end: Date.UTC(2026, 2, 17),
        upTop: [{ key: '10000073', names: { 'zh-cn': '纳西妲', 'en-us': 'Nahida' } }],
        upSecond: [{ key: '10000023', names: { 'zh-cn': '香菱', 'en-us': 'Xiangling' } }],
        pulls: 10,
        tops: [STATS.pools[0]!.tops[1]!],
      },
      {
        kind: 'banner',
        id: 'b2',
        names: {},
        rawName: null,
        start: Date.UTC(2026, 1, 4),
        end: Date.UTC(2026, 1, 25),
        upTop: [{ key: '10000002', names: { 'zh-cn': '神里绫华', 'en-us': 'Kamisato Ayaka' } }],
        upSecond: [],
        pulls: 0,
        tops: [],
      },
      {
        kind: 'banner',
        id: 'b1',
        names: {},
        rawName: null,
        start: Date.UTC(2026, 0, 14),
        end: Date.UTC(2026, 1, 4),
        upTop: [{ key: '10000022', names: { 'zh-cn': '温迪', 'en-us': 'Venti' } }],
        upSecond: [],
        pulls: 20,
        tops: [STATS.pools[0]!.tops[0]!],
      },
      {
        kind: 'auto',
        id: null,
        names: {},
        rawName: null,
        start: Date.UTC(2026, 0, 10),
        end: Date.UTC(2026, 0, 10, 2),
        upTop: [],
        upSecond: [],
        pulls: 10,
        tops: [],
      },
    ],
  }
}
