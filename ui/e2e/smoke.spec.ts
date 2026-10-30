// 冒烟测试（T-41、T-53）：首次导入、总览、分析图表、主题与语言切换。
import { expect, test, type Page } from '@playwright/test'

/** 打开应用；`empty` 表示从没有账号开始。 */
async function open(page: Page, hash = '/', empty = false) {
  await page.goto(`/e2e/index.html${empty ? '?empty' : ''}#${hash}`)
}

test('首次导入：空状态 → 选文件 → 预览 → 进度 → 报告 → 总览出现账号', async ({ page }) => {
  await open(page, '/', true)
  await expect(page.getByTestId('overview-empty')).toBeVisible()

  await page.getByTestId('open-import').click()
  await page.getByTestId('pick-genshin').click()
  await page.getByTestId('method-file').click()
  await page.getByTestId('choose-file').click()
  await expect(page.getByTestId('preview-table')).toContainText('100000001')
  await page.getByTestId('start-file').click()

  const report = page.getByTestId('report')
  await expect(report).toBeVisible()
  await expect(report).toContainText('52')
  await expect(page.getByTestId('import-wizard')).toContainText('auto-20261026.db')
  await page.getByRole('button', { name: '完成' }).click()
  await expect(page.getByTestId('import-wizard')).toBeHidden()

  // 导入后边栏出现原神，总览出现账号卡片
  await expect(page.getByTestId('nav-genshin')).toBeVisible()
  await expect(page.getByTestId('overview-card')).toHaveCount(1)
  await expect(page.getByTestId('overview-total')).toHaveText('52')
})

test('总览：卡片数字来自后端，点进账号统计页', async ({ page }) => {
  await open(page)
  const card = page.getByTestId('overview-card')
  await expect(card).toHaveCount(1)
  await expect(card).toContainText('1****0001')
  await expect(page.getByTestId('overview-total')).toHaveText('52')

  await page.getByTestId('open-account').click()
  await expect(page).toHaveURL(/#\/account\/acc-1\/stats$/)
  await expect(page.getByTestId('account-metrics')).toContainText('87.0')
  await expect(page.getByTestId('account-loss')).toHaveText('100.0%')
})

test('主题：边栏按钮在浅色与深色之间切换', async ({ page }) => {
  await open(page)
  const html = page.locator('html')
  await expect(html).toHaveAttribute('data-theme', 'light')
  await page.getByTestId('toggle-theme').click()
  await expect(html).toHaveAttribute('data-theme', 'dark')
  // 背景色确实变了（主题变量生效）
  const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor)
  await page.getByTestId('toggle-theme').click()
  await expect(html).toHaveAttribute('data-theme', 'light')
  expect(await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).not.toBe(bg)
})

test('语言：设置里切到 English，界面文案随之变化', async ({ page }) => {
  await open(page, '/settings/general')
  await expect(page.getByRole('link', { name: '总览' })).toBeVisible()
  await page.getByRole('tab', { name: 'English' }).click()
  await expect(page.getByRole('link', { name: 'Overview' })).toBeVisible()
  await expect(page.locator('html')).toHaveAttribute('lang', 'en')
  await page.getByRole('tab', { name: '简体中文' }).click()
  await expect(page.getByRole('link', { name: '总览' })).toBeVisible()
})

test('分析：图表按需加载并跟随主题配色', async ({ page }) => {
  await open(page, '/account/acc-1/stats')
  await page.getByTestId('tab-analysis').click()
  await expect(page).toHaveURL(/#\/account\/acc-1\/analysis$/)
  await expect(page.getByTestId('next-top')).toHaveText('0.6%')
  // 三张图（预测曲线、出金分布、月度趋势）都渲染成 SVG
  const charts = page.locator('.echart svg')
  await expect(charts).toHaveCount(3)
  const bars = page.getByTestId('distribution').locator('svg path[fill="#1f7a4d"]')
  await expect(bars).toHaveCount(1)
  // 深色主题下 UP 柱子换成深色主题的颜色
  await page.getByTestId('toggle-theme').click()
  await expect(page.getByTestId('distribution').locator('svg path[fill="#5ccb94"]')).toHaveCount(1)
  // 切换为数据表
  await page.getByTestId('toggle-dist').click()
  await expect(page.getByTestId('distribution').locator('table')).toContainText('纳西妲')
})
