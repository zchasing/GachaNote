import { defineConfig, devices } from '@playwright/test'

// 冒烟测试（T-41）：用 Vite 开发服务器加载真实前端，后端由 e2e/mock-backend.ts 模拟。
// 本地运行：pnpm exec playwright install chromium（只需一次），然后 pnpm e2e。
const port = 1421

export default defineConfig({
  testDir: 'e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://localhost:${port}`,
    locale: 'zh-CN',
    testIdAttribute: 'data-test',
    trace: 'retain-on-failure',
  },
  // 与安装包一致：WebView2 基于 Chromium，窗口默认 1280 × 800
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 } } }],
  webServer: {
    command: `pnpm exec vite --port ${port} --strictPort`,
    url: `http://localhost:${port}/e2e/index.html`,
    reuseExistingServer: !process.env.CI,
  },
})
