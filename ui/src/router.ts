import { createRouter, createWebHashHistory, type RouterHistory } from 'vue-router'

// 侧边栏导航对应路由（架构文档第 8 节）。账号详情的标签页是子路由，切换标签不重置所选卡池。
export function createAppRouter(history: RouterHistory = createWebHashHistory()) {
  return createRouter({
    history,
    routes: [
      { path: '/', name: 'overview', component: () => import('./views/OverviewView.vue') },
      { path: '/game/:game', name: 'game', component: () => import('./views/GameView.vue') },
      {
        path: '/account/:id',
        component: () => import('./views/AccountView.vue'),
        children: [
          { path: '', redirect: (to) => `/account/${String(to.params.id)}/stats` },
          { path: 'stats', name: 'stats', component: () => import('./views/StatsTab.vue') },
          { path: 'records', name: 'records', component: () => import('./views/RecordsTab.vue') },
        ],
      },
      {
        path: '/settings/:section?',
        name: 'settings',
        component: () => import('./views/SettingsView.vue'),
      },
      { path: '/:pathMatch(.*)*', redirect: '/' },
    ],
  })
}
