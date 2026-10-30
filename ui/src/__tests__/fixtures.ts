// 前端测试用的 IPC 模拟与挂载工具。
import { mockIPC } from '@tauri-apps/api/mocks'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import { createMemoryHistory } from 'vue-router'
import type { AccountDto } from '@/bindings'
import App from '@/App.vue'
import { i18n, setLocale } from '@/i18n'
import { createAppRouter } from '@/router'
import { ACCOUNT, GAMES, MONTHS, PAGE, SETTINGS, STATS, prediction, timeline } from './data'

export { ACCOUNT, GAMES, MONTHS, PAGE, SETTINGS, STATS, prediction, timeline }

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
      canRememberToken: true,
    }),
    list_backups: () => [],
    meta_info: () => ({ version: '2026.10.1', origin: 'bundled' }),
    predict: (a) => prediction(Number(a.pulls)),
    get_monthly_trend: () => MONTHS,
    get_timeline: () => timeline(),
    get_item_icons: () => ({}),
    list_credentials: () => [],
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
