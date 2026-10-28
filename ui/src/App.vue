<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { events, inTauri } from '@/api'
import AppSidebar from '@/components/AppSidebar.vue'
import ConfirmDialog from '@/components/ConfirmDialog.vue'
import ImportWizard from '@/components/ImportWizard.vue'
import ToastHost from '@/components/ToastHost.vue'
import UpdateDialog from '@/components/UpdateDialog.vue'
import { useErrors } from '@/composables/useErrors'
import { useTheme } from '@/composables/useTheme'
import { resolveLocale, setLocale } from '@/i18n'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useSettingsStore } from '@/stores/settings'
import { useTasksStore } from '@/stores/tasks'
import { useUiStore } from '@/stores/ui'
import { useUpdaterStore } from '@/stores/updater'

const settings = useSettingsStore()
const games = useGamesStore()
const accounts = useAccountsStore()
const tasks = useTasksStore()
const ui = useUiStore()
const updater = useUpdaterStore()
const route = useRoute()
const router = useRouter()
const { report } = useErrors()
useTheme()

const ready = ref(false)
/** 1024–1279 px 时边栏默认收起到 64 px（UI/UX 文档第 7 节）；用户可手动切换。 */
const narrow = typeof matchMedia === 'function' ? matchMedia('(max-width: 1279px)') : null
const collapsed = ref(narrow?.matches ?? false)
const onNarrow = (e: MediaQueryListEvent) => (collapsed.value = e.matches)

watch(
  () => settings.settings.language,
  (lang) => setLocale(resolveLocale(lang)),
  { immediate: true },
)

const unlisten: (() => void)[] = []

onMounted(async () => {
  narrow?.addEventListener('change', onNarrow)
  window.addEventListener('keydown', onKey)
  try {
    if (inTauri()) {
      await tasks.listen()
      unlisten.push(
        await events.dataChanged.listen(({ payload }) => void accounts.invalidate(payload.accountIds)),
        await events.metaUpdated.listen(() => void accounts.invalidate([])),
      )
    }
    await Promise.all([settings.load(), games.load(), accounts.load()])
  } catch (e) {
    report(e)
  } finally {
    ready.value = true
  }
  // 启动时在后台检查软件新版本（F-24），失败不打扰
  if (settings.settings.updateAutoCheck) void updater.check(true)
})

onBeforeUnmount(() => {
  narrow?.removeEventListener('change', onNarrow)
  window.removeEventListener('keydown', onKey)
  unlisten.forEach((f) => f())
})

/** 快捷键（UI/UX 文档第 6 节）。Ctrl+R 在 WebView 里默认是刷新页面，这里改为更新当前账号。 */
function onKey(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey) || e.altKey) return
  const accountId = typeof route.params.id === 'string' ? route.params.id : undefined
  switch (e.key.toLowerCase()) {
    case 'i':
      e.preventDefault()
      ui.openImport()
      break
    case 'r':
      e.preventDefault()
      if (accountId) {
        const game = accounts.account(accountId)?.game
        if (game) ui.openImport({ game })
      }
      break
    case ',':
      e.preventDefault()
      void router.push('/settings')
      break
    case 'f':
      if (accountId) {
        e.preventDefault()
        void router.push(`/account/${accountId}/records`).then(() => {
          document.querySelector<HTMLInputElement>('[data-test="record-search"]')?.focus()
        })
      }
      break
    case '1':
    case '2':
    case '3':
      if (accountId) {
        e.preventDefault()
        const tab = { '1': 'stats', '2': 'records', '3': 'analysis' }[e.key]
        void router.push(`/account/${accountId}/${tab}`)
      }
      break
  }
}
</script>

<template>
  <div class="app">
    <AppSidebar :collapsed="collapsed" @toggle="collapsed = !collapsed" />
    <main class="main">
      <RouterView v-if="ready" />
    </main>
    <ImportWizard v-if="ui.importRequest" :request="ui.importRequest" @close="ui.closeImport()" />
    <ConfirmDialog />
    <UpdateDialog />
    <ToastHost />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  height: 100vh;
  min-height: 600px;
  background: var(--bg);
}
.main {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}
</style>
