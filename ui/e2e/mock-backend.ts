// Playwright 冒烟测试用的模拟后端：在浏览器里拦截 Tauri IPC，按真实命令的形状返回数据，
// 导入任务会依次推送进度与完成事件。URL 参数 `?empty` 表示从没有账号的状态开始。
import { emit } from '@tauri-apps/api/event'
import { mockIPC } from '@tauri-apps/api/mocks'
import type { AccountDto, FilePreviewDto, ImportReportDto, Settings } from '@/bindings'
import { ACCOUNT, GAMES, MONTHS, PAGE, SETTINGS, STATS, prediction } from '@/__tests__/data'

const params = new URLSearchParams(location.search)
let accounts: AccountDto[] = params.has('empty') ? [] : [ACCOUNT]
let settings: Settings = { ...SETTINGS }
let nextTask = 1

const FILE = 'C:\\Users\\me\\Downloads\\uigf.json'
const PREVIEW: FilePreviewDto = {
  path: FILE,
  fileName: 'uigf.json',
  format: 'uigf_v4',
  accounts: [
    {
      game: 'genshin',
      uid: ACCOUNT.uid,
      count: STATS.totalPulls,
      failed: 0,
      from: STATS.firstAt,
      to: STATS.lastAt,
      exists: false,
      supported: true,
    },
  ],
}

/** 模拟后台导入：解析 → 备份 → 写入，然后报告并通知数据变化。 */
function runImport(task: number) {
  const steps = ['parse', 'backup', 'write'] as const
  steps.forEach((stage, i) => {
    setTimeout(() => {
      void emit('task://progress', { taskId: task, stage, current: i + 1, total: steps.length, message: '' })
    }, 50 * (i + 1))
  })
  setTimeout(() => {
    accounts = [{ ...ACCOUNT, lastImportAt: Date.now() }]
    const report: ImportReportDto = {
      accounts: [
        {
          accountId: ACCOUNT.id,
          game: 'genshin',
          uid: ACCOUNT.uid,
          created: true,
          added: STATS.totalPulls,
          skipped: 0,
          failed: 0,
        },
      ],
      failures: [],
      added: STATS.totalPulls,
      skipped: 0,
      failed: 0,
      backup: 'auto-20261026.db',
    }
    void emit('task://done', { taskId: task, report })
    void emit('data://changed', { accountIds: [ACCOUNT.id] })
  }, 50 * (steps.length + 1))
}

const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  list_games: () => GAMES,
  list_accounts: () => accounts,
  get_settings: () => settings,
  set_settings: (a) => {
    settings = a.settings as Settings
    return null
  },
  get_stats: () => STATS,
  get_overview: () => accounts.map((account) => ({ account, stats: STATS })),
  query_records: () => PAGE,
  app_info: () => ({
    version: '0.1.0',
    dataDir: 'C:\\Users\\me\\AppData\\Roaming\\GachaNote',
    defaultDataDir: 'C:\\Users\\me\\AppData\\Roaming\\GachaNote',
    logDir: 'C:\\Users\\me\\AppData\\Roaming\\GachaNote\\logs',
    dbSize: 4096,
    canDiscover: false,
    canRememberToken: true,
  }),
  list_backups: () => [],
  meta_info: () => ({ version: '2026.10.1', origin: 'bundled' }),
  predict: (a) => prediction(Number(a.pulls)),
  get_monthly_trend: () => MONTHS,
  list_credentials: () => [],
  check_update: () => null,
  'plugin:dialog|open': () => FILE,
  sniff_file: () => PREVIEW,
  import_file: () => {
    const task = nextTask++
    runImport(task)
    return task
  },
}

mockIPC(
  (cmd, args) => {
    const h = handlers[cmd]
    if (!h) throw { code: 'internal', message: `e2e 模拟后端没有实现 ${cmd}`, detail: null }
    return h((args ?? {}) as Record<string, unknown>)
  },
  { shouldMockEvents: true },
)
