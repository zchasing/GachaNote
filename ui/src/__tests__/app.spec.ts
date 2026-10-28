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

  it('鹰角：粘贴 token → 选择角色 → 记住 token 拉取', async () => {
    const ipc = setupIpc({
      hg_roles: () => [
        { game: 'arknights', uid: '12345678', nickname: '博士#1234', channel: '官服', exists: false },
        { game: 'endfield', uid: '1000000005', nickname: '管理员', channel: '官服', exists: false },
      ],
      fetch_by_token: () => 11,
    })
    const { wrapper: w } = await start('/', ipc)
    await w.get('[data-test="open-import"]').trigger('click')
    await w.get('[data-test="pick-arknights"]').trigger('click')
    await settle()
    // 鹰角游戏没有链接拉取；没有保存的 token 时不显示「用已保存的 token」
    expect(w.find('[data-test="method-url"]').exists()).toBe(false)
    expect(w.find('[data-test="method-saved"]').exists()).toBe(false)
    await w.get('[data-test="method-token"]').trigger('click')
    await settle()
    await w.get('[data-test="paste-token"]').setValue(' token-0123456789abcdef ')
    await w.get('[data-test="query-roles"]').trigger('click')
    await settle()
    expect(ipc.called('hg_roles')[0]!.args).toEqual({ source: { kind: 'pasted', token: 'token-0123456789abcdef' } })
    expect(w.get('[data-test="role-table"]').text()).toContain('博士#1234')
    // 默认全选、默认记住
    expect((w.get('[data-test="remember-token"]').element as HTMLInputElement).checked).toBe(true)
    const startBtn = w.get('[data-test="start-token"]')
    expect(startBtn.text()).toBe('拉取 2 个角色')
    await w.findAll('[data-test="role-table"] input[type="checkbox"]')[1]!.trigger('change')
    await w.get('[data-test="start-token"]').trigger('click')
    await settle()
    expect(ipc.called('fetch_by_token')[0]!.args).toEqual({
      source: { kind: 'pasted', token: 'token-0123456789abcdef' },
      roles: [{ game: 'arknights', uid: '12345678' }],
      remember: true,
      mode: 'full',
    })
    await emit('task://failed', {
      taskId: 11,
      error: { code: 'auth_expired', message: 'expired', detail: null },
    })
    await settle()
    expect(w.get('[data-test="report-error"]').text()).toContain('token 无效或已过期')
  })

  it('鹰角：关闭登录窗口后提示改用粘贴', async () => {
    const ipc = setupIpc({
      hg_login: () => {
        throw { code: 'cancelled', message: 'cancelled', detail: null }
      },
    })
    const { wrapper: w } = await start('/', ipc)
    await w.get('[data-test="open-import"]').trigger('click')
    await w.get('[data-test="pick-arknights"]').trigger('click')
    await settle()
    await w.get('[data-test="method-login"]').trigger('click')
    await w.get('[data-test="hg-login"]').trigger('click')
    await settle()
    const args = ipc.called('hg_login')[0]!.args as { title: string; hint: string }
    expect(args.hint).toContain('不接触密码')
    expect(w.get('[data-test="login-error"]').text()).toContain('登录窗口已关闭')
    await w.get('[data-test="login-error"] button').trigger('click')
    await settle()
    expect(w.find('[data-test="paste-token"]').exists()).toBe(true)
  })

  it('鹰角：用已保存的 token 一键更新', async () => {
    const saved = { accountId: 'acc-ak', game: 'arknights', uid: '12345678', nickname: null, kind: 'hg_account_token', updatedAt: 1 }
    const ipc = setupIpc({
      list_credentials: () => [saved],
      hg_roles: () => [{ game: 'arknights', uid: '12345678', nickname: null, channel: null, exists: true }],
      fetch_by_token: () => 12,
    })
    const { wrapper: w } = await start('/', ipc)
    await w.get('[data-test="open-import"]').trigger('click')
    await w.get('[data-test="pick-arknights"]').trigger('click')
    await settle()
    await w.get('[data-test="method-saved"]').trigger('click')
    await settle()
    expect(ipc.called('hg_roles')[0]!.args).toEqual({ source: { kind: 'saved', accountId: 'acc-ak' } })
    // 已保存的 token 不再显示「记住」
    expect(w.find('[data-test="remember-token"]').exists()).toBe(false)
    await w.get('[data-test="start-token"]').trigger('click')
    await settle()
    expect(ipc.called('fetch_by_token')[0]!.args).toMatchObject({ remember: false })
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

describe('分析', () => {
  it('概率预测、欧非、分布、月度与成就', async () => {
    const { wrapper: w, ipc } = await start('/account/acc-1/analysis')
    await settle()
    expect(w.get('[data-test="next-top"]').text()).toBe('0.6%')
    expect(w.get('[data-test="next-up"]').text()).toBe('0.3%')
    expect(ipc.called('predict')[0]!.args).toEqual({ accountId: 'acc-1', poolType: 'character', pulls: 10 })
    expect(w.get('[data-test="within"]').text()).toContain('出 5 星')
    // 样本少于 5 个金时只显示数值
    expect(w.find('[data-test="luck-tier"]').exists()).toBe(false)
    expect(w.get('[data-test="luck-few"]').text()).toContain('暂不评级')
    expect(w.get('[data-test="luck"]').text()).toContain('93.5')
    expect(w.get('[data-test="ach-lucky"]').classes()).toContain('done')
    expect(w.get('[data-test="ach-streak"]').classes()).not.toContain('done')
    expect(w.get('[data-test="ach-lucky"]').text()).toContain('刻晴')

    // 图表都能切换为数据表
    await w.get('[data-test="toggle-dist"]').trigger('click')
    await w.get('[data-test="toggle-month"]').trigger('click')
    await w.get('[data-test="toggle-curve"]').trigger('click')
    await settle()
    expect(w.get('[data-test="distribution"]').text()).toContain('纳西妲')
    expect(w.get('[data-test="monthly"]').text()).toContain('2026-03')
    expect(w.get('[data-test="prediction"] table').text()).toContain('10')

    await w.get('[data-test="within-n"]').setValue(50)
    await settle()
    expect(ipc.called('predict').at(-1)!.args).toMatchObject({ pulls: 50 })
  })

  it('百分位够样本时显示评级，统计页有一句话结论', async () => {
    const pools = STATS.pools.map((p) =>
      p.poolType === 'character' ? { ...p, luck: { ...p.luck!, samples: 6, percentile: 0.78 } } : p,
    )
    const ipc = setupIpc({ get_stats: () => ({ ...STATS, pools }) })
    const { wrapper: w } = await start('/account/acc-1/analysis', ipc)
    await settle()
    expect(w.get('[data-test="luck-tier"]').text()).toBe('欧')
    expect(w.get('[data-test="luck"]').text()).toContain('比 78.0% 的人运气好')

    const { wrapper: s } = await start('/account/acc-1/stats', ipc)
    await s.findAll('[data-test="pool-row"]')[0]!.trigger('click')
    await settle()
    expect(s.get('[data-test="verdict"]').text()).toContain('下一抽出 5 星 0.6% · 欧 · 前 22.0%')
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

  it('凭证页列出并清除保存的 token', async () => {
    let creds = [
      { accountId: 'acc-ak', game: 'arknights', uid: '12345678', nickname: '博士', kind: 'hg_account_token', updatedAt: 1 },
    ]
    const ipc = setupIpc({
      list_credentials: () => creds,
      clear_credentials: () => {
        creds = []
        return 1
      },
    })
    const { wrapper: w } = await start('/settings/credentials', ipc)
    expect(w.findAll('[data-test="credential-row"]')).toHaveLength(1)
    expect(w.get('[data-test="credentials"]').text()).not.toContain('token-')
    await w.get('[data-test="clear-all-credentials"]').trigger('click')
    await settle()
    await w.get('[data-test="confirm-ok"]').trigger('click')
    await settle()
    expect(ipc.called('clear_credentials')[0]!.args).toEqual({ accountId: null })
    expect(w.findAll('[data-test="credential-row"]')).toHaveLength(0)
  })

  it('检查更新：有新版本时弹窗，未配置时引导去发布页', async () => {
    let configured = true
    const ipc = setupIpc({
      check_update: () => {
        if (!configured) throw { code: 'invalid', message: 'updater is not configured', detail: 'updater is not configured' }
        return { version: '0.2.0', currentVersion: '0.1.0', date: null, notes: '- 分析页' }
      },
    })
    const { wrapper: w } = await start('/settings/about', ipc)
    await w.get('[data-test="update-check"]').trigger('click')
    await settle()
    expect(w.get('[data-test="update-dialog"]').text()).toContain('v0.2.0')
    await w.get('[data-test="update-install"]').trigger('click')
    await settle()
    expect(ipc.called('install_update')).toHaveLength(1)
    w.unmount()

    configured = false
    const { wrapper: w2 } = await start('/settings/about', ipc)
    wrapper = w2
    await w2.get('[data-test="update-check"]').trigger('click')
    await settle()
    expect(w2.get('[data-test="update-status"]').text()).toContain('发布页')
  })

  it('数据页显示目录与账号', async () => {
    const { wrapper: w } = await start('/settings/data')
    expect(w.get('[data-test="data-dir"]').text()).toBe('/data/GachaNote')
    expect(w.findAll('[data-test="settings-account"]')).toHaveLength(1)
    expect(STATS.totalPulls).toBe(52)
  })
})
