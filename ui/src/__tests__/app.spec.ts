import { emit } from '@tauri-apps/api/event'
import { clearMocks } from '@tauri-apps/api/mocks'
import type { VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ACCOUNT, mountApp, setupIpc, settle, STATS } from './fixtures'

let wrapper: VueWrapper | undefined
afterEach(() => {
  wrapper?.unmount()
  wrapper = undefined
  clearMocks()
  document.body.innerHTML = ''
})

async function start(path: string, ipc = setupIpc()) {
  const m = await mountApp(path)
  wrapper = m.wrapper
  return { ...m, ipc }
}

describe('边栏', () => {
  it('只显示有账号的游戏，其余游戏收在「添加游戏」里', async () => {
    const { wrapper: w, ipc } = await start('/')
    expect(w.find('[data-test="nav-genshin"]').exists()).toBe(true)
    expect(w.find('[data-test="nav-starrail"]').exists()).toBe(false)

    await w.get('[data-test="add-game"]').trigger('click')
    await w.get('[data-test="add-starrail"]').trigger('click')
    await settle()
    expect(w.find('[data-test="nav-starrail"]').exists()).toBe(true)
    const saved = ipc.called('set_settings').at(-1)!.args.settings as { sidebarGames: string[] }
    expect(saved.sidebarGames).toEqual(['starrail'])
  })

  it('导入后出现新游戏', async () => {
    const { wrapper: w, ipc } = await start('/')
    ipc.setAccounts([ACCOUNT, { ...ACCOUNT, id: 'acc-2', game: 'arknights', uid: '42' }])
    await emit('data://changed', { accountIds: ['acc-2'] })
    await settle()
    expect(w.find('[data-test="nav-arknights"]').exists()).toBe(true)
  })
})

describe('总览', () => {
  it('每个账号一张卡片，数字来自后端', async () => {
    const { wrapper: w } = await start('/')
    const cards = w.findAll('[data-test="overview-card"]')
    expect(cards).toHaveLength(1)
    expect(cards[0]!.text()).toContain('1****0001')
    expect(w.get('[data-test="overview-total"]').text()).toBe('52')
    // 默认只显示有记录的 UP 卡池（角色池）
    expect(w.findAll('[data-test="overview-pool"]')).toHaveLength(1)
  })

  it('没有账号时显示空状态', async () => {
    const ipc = setupIpc()
    ipc.setAccounts([])
    const { wrapper: w } = await start('/', ipc)
    expect(w.find('[data-test="overview-empty"]').exists()).toBe(true)
  })

  it('点卡片进入账号统计', async () => {
    const { wrapper: w, router } = await start('/')
    await w.get('[data-test="open-account"]').trigger('click')
    // 账号页是懒加载路由，首次加载要等模块转换完
    await vi.waitFor(() => expect(router.currentRoute.value.fullPath).toBe('/account/acc-1/stats'))
  })
})

describe('统计', () => {
  it('账号总览显示合计与估算标记', async () => {
    const { wrapper: w } = await start('/account/acc-1/stats')
    const metrics = w.get('[data-test="account-metrics"]').text()
    expect(metrics).toContain('52')
    expect(metrics).toContain('87.0')
    expect(metrics).toContain('估算')
    expect(w.get('[data-test="account-loss"]').text()).toBe('100.0%')
    // 没有记录的武器池不出现
    expect(w.findAll('[data-test="pool-row"]')).toHaveLength(2)
  })

  it('单卡池显示保底与出金历史', async () => {
    const { wrapper: w } = await start('/account/acc-1/stats')
    await w.findAll('[data-test="pool-row"]')[0]!.trigger('click')
    await settle()
    expect(w.get('[data-test="pity"]').text()).toContain('3')
    expect(w.get('[data-test="pool-metrics"]').text()).toContain('距保底还剩 87 抽')
    const history = w.get('[data-test="history"]').text()
    // 新的在上
    expect(history.indexOf('纳西妲')).toBeLessThan(history.indexOf('刻晴'))
    expect(history).toContain('歪')
    expect(w.get('[data-test="metric-loss"]').text()).toContain('1 / 1')
  })

  it('访问不存在的账号时提示', async () => {
    const { wrapper: w } = await start('/account/missing/stats')
    expect(w.text()).toContain('找不到这个账号')
  })
})

describe('记录', () => {
  it('显示当次抽数与 UP 标记，筛选会重新查询并回到第一页', async () => {
    const { wrapper: w, ipc } = await start('/account/acc-1/records')
    const rows = w.findAll('[data-test="record-row"]')
    expect(rows).toHaveLength(2)
    expect(rows[0]!.text()).toContain('77')
    expect(rows[0]!.text()).toContain('UP')
    expect(rows[0]!.text()).toContain('2026-03-01 12:05')

    await w.get('[data-test="only-top"]').setValue(true)
    await settle()
    const q = ipc.called('query_records').at(-1)!.args.query as { rarities: number[]; page: number }
    expect(q.rarities).toEqual([5])
    expect(q.page).toBe(0)
  })

  it('名称搜索防抖后查询', async () => {
    const { wrapper: w, ipc } = await start('/account/acc-1/records')
    const before = ipc.called('query_records').length
    await w.get('[data-test="record-search"]').setValue('纳西')
    await new Promise((r) => setTimeout(r, 300))
    await settle()
    const after = ipc.called('query_records')
    expect(after.length).toBe(before + 1)
    expect((after.at(-1)!.args.query as { name: string }).name).toBe('纳西')
  })
})

describe('账号操作', () => {
  it('删除前确认并显示记录数', async () => {
    const { wrapper: w, ipc, router } = await start('/account/acc-1/stats')
    await w.get('[data-test="account-more"]').trigger('click')
    await w.get('[data-test="delete-account"]').trigger('click')
    await settle()
    const ok = w.get('[data-test="confirm-ok"]')
    expect(ok.text()).toBe('删除 52 条记录')
    ipc.setAccounts([])
    await ok.trigger('click')
    await settle()
    expect(ipc.called('delete_account')[0]!.args).toEqual({ accountId: 'acc-1' })
    // 游戏页是懒加载路由，CI 上首次加载可能晚于 settle()
    await vi.waitFor(() => expect(router.currentRoute.value.fullPath).toBe('/game/genshin'))
  })
})

describe('导入向导', () => {
  it('文件导入：预览 → 导入 → 报告', async () => {
    const ipc = setupIpc({
      'plugin:dialog|open': () => '/tmp/uigf.json',
      sniff_file: () => ({
        path: '/tmp/uigf.json',
        fileName: 'uigf.json',
        format: 'uigf_v4',
        accounts: [
          { game: 'genshin', uid: '100000001', count: 52, failed: 0, from: 1, to: 2, exists: true, supported: true },
          { game: 'starrail', uid: '200000001', count: 10, failed: 0, from: 1, to: 2, exists: false, supported: true },
        ],
      }),
      import_file: () => 7,
    })
    const { wrapper: w } = await start('/', ipc)
    await w.get('[data-test="open-import"]').trigger('click')
    await w.get('[data-test="pick-genshin"]').trigger('click')
    await w.get('[data-test="method-file"]').trigger('click')
    await w.get('[data-test="choose-file"]').trigger('click')
    await settle()
    expect(w.get('[data-test="preview-table"]').text()).toContain('已有账号，合并')
    // 选了原神，只默认选中原神账号
    const start1 = w.get('[data-test="start-file"]')
    expect(start1.text()).toBe('导入选中的 1 个账号')
    await start1.trigger('click')
    await settle()
    expect(ipc.called('import_file')[0]!.args).toEqual({
      path: '/tmp/uigf.json',
      only: [{ game: 'genshin', uid: '100000001' }],
    })
    expect(w.find('[data-test="progress"]').exists()).toBe(true)

    await emit('task://done', {
      taskId: 7,
      report: {
        accounts: [
          { accountId: 'acc-1', game: 'genshin', uid: '100000001', created: false, added: 3, skipped: 49, failed: 0 },
        ],
        failures: [],
        added: 3,
        skipped: 49,
        failed: 0,
        backup: 'auto-20261026.db',
      },
    })
    await settle()
    const report = w.get('[data-test="report"]').text()
    expect(report).toContain('3')
    expect(report).toContain('49')
    expect(w.text()).toContain('auto-20261026.db')
  })

  it('鹰角游戏没有链接拉取，账号登录显示为 v0.2', async () => {
    const { wrapper: w } = await start('/')
    await w.get('[data-test="open-import"]').trigger('click')
    await w.get('[data-test="pick-arknights"]').trigger('click')
    expect(w.find('[data-test="method-url"]').exists()).toBe(false)
    expect(w.get('[data-test="import-wizard"]').text()).toContain('v0.2')
  })

  it('粘贴链接拉取，失败时显示错误与重试提示', async () => {
    const ipc = setupIpc({
      fetch_by_url: () => 9,
    })
    const { wrapper: w } = await start('/', ipc)
    await w.get('[data-test="open-import"]').trigger('click')
    await w.get('[data-test="pick-genshin"]').trigger('click')
    await w.get('[data-test="method-url"]').trigger('click')
    await settle()
    await w.get('[data-test="paste-url"]').setValue('https://example.com/gacha?authkey=x')
    await w.get('[data-test="start-url"]').trigger('click')
    await settle()
    expect(ipc.called('fetch_by_url')[0]!.args).toEqual({
      game: 'genshin',
      source: { kind: 'pasted', url: 'https://example.com/gacha?authkey=x' },
      mode: 'incremental',
    })
    await emit('task://failed', {
      taskId: 9,
      error: { code: 'auth_expired', message: 'authkey timeout', detail: null },
    })
    await settle()
    expect(w.get('[data-test="report-error"]').text()).toContain('抽卡链接已过期')
  })
})

describe('设置', () => {
  it('切换主题写回设置并更新页面', async () => {
    const { wrapper: w, ipc } = await start('/settings/general')
    const dark = w.findAll('button').find((b) => b.text() === '深色')!
    await dark.trigger('click')
    await settle()
    expect((ipc.called('set_settings').at(-1)!.args.settings as { theme: string }).theme).toBe('dark')
    expect(document.documentElement.dataset.theme).toBe('dark')
  })

  it('数据页显示目录与账号', async () => {
    const { wrapper: w } = await start('/settings/data')
    expect(w.get('[data-test="data-dir"]').text()).toBe('/data/GachaNote')
    expect(w.findAll('[data-test="settings-account"]')).toHaveLength(1)
    expect(STATS.totalPulls).toBe(52)
  })
})
