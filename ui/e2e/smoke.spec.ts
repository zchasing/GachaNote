// 冒烟测试（T-41、T-53、T-61、T-62）：首次导入、总览、分析图表、时间线、分享图、主题与语言切换。
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

test('时间线：按日堆叠图、拖动选区与卡池期数', async ({ page }) => {
  await open(page, '/account/acc-1/timeline')
  await expect(page.getByTestId('day-range')).toContainText('2026-01-10 – 2026-03-10')
  // 三档星级堆叠，金色柱子有两天
  await expect(page.getByTestId('timeline-chart').locator('svg path[fill="#f5b83d"]')).toHaveCount(2)
  await expect(page.getByTestId('period')).toHaveCount(3)

  // 按住右手柄向左拖到一半，选区缩小
  const brush = await page.getByTestId('brush').boundingBox()
  const handle = await page.getByTestId('brush-end').boundingBox()
  await page.mouse.move(handle!.x + handle!.width / 2, handle!.y + handle!.height / 2)
  await page.mouse.down()
  await page.mouse.move(brush!.x + brush!.width / 2, handle!.y + handle!.height / 2, { steps: 5 })
  await page.mouse.up()
  await expect(page.getByTestId('selected-range')).toContainText('2026-01-10 – 2026-02-0')
  await expect(page.getByTestId('pan-right')).toBeEnabled()

  await page.getByTestId('quick-all').click()
  await expect(page.getByTestId('pan-right')).toBeDisabled()
})

test('分享图：画布按 1080 px 绘制，可切深色并保存', async ({ page }) => {
  await open(page, '/account/acc-1/stats')
  await page.getByTestId('account-more').click()
  await page.getByTestId('open-share').click()
  const canvas = page.getByTestId('share-canvas')
  await expect(canvas).toHaveAttribute('aria-label', /UID 1\*\*\*\*0001/)
  const pixel = () =>
    canvas.evaluate((c: HTMLCanvasElement) => {
      const d = c.getContext('2d')!.getImageData(4, 4, 1, 1).data
      return { width: c.width, rgb: [d[0], d[1], d[2]] }
    })
  expect(await pixel()).toEqual({ width: 1080, rgb: [247, 247, 248] })
  await page.getByRole('tab', { name: '深色' }).click()
  await expect.poll(async () => (await pixel()).rgb).toEqual([20, 21, 24])
  await page.getByTestId('share-save').click()
  await expect(page.getByText('已保存分享图')).toBeVisible()
})
