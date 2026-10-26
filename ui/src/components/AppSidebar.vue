<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useSettingsStore } from '@/stores/settings'
import { useUiStore } from '@/stores/ui'
import { useNames } from '@/composables/useNames'
import { useTheme } from '@/composables/useTheme'
import AppIcon from './AppIcon.vue'
import AppLogo from './AppLogo.vue'
import GameIcon from './GameIcon.vue'

// 左侧边栏（UI/UX 文档 3.1）：只列游戏，没有账号的游戏默认隐藏；「+ 添加游戏」展开其余游戏。
defineProps<{ collapsed: boolean }>()
const emit = defineEmits<{ toggle: [] }>()

const route = useRoute()
const games = useGamesStore()
const accounts = useAccountsStore()
const settings = useSettingsStore()
const ui = useUiStore()
const { t, gameName } = useNames()
const { isDark, toggle: toggleTheme } = useTheme()
const version = __APP_VERSION__

const counts = computed(() => {
  const m = new Map<string, number>()
  for (const a of accounts.accounts) m.set(a.game, (m.get(a.game) ?? 0) + 1)
  return m
})

/** 显示的游戏：有账号的，以及用户手动添加的；顺序跟随游戏列表。 */
const shown = computed(() =>
  games.games.filter((g) => counts.value.has(g.id) || settings.settings.sidebarGames.includes(g.id)),
)
const hidden = computed(() => games.games.filter((g) => !shown.value.includes(g)))

const adding = ref(false)
const menu = ref<{ game: string; x: number; y: number } | null>(null)

function addGame(id: string) {
  adding.value = false
  void settings.update({ sidebarGames: [...settings.settings.sidebarGames, id] })
}

function onContext(e: MouseEvent, id: string) {
  // 只能隐藏没有账号的游戏
  if (counts.value.has(id)) return
  e.preventDefault()
  menu.value = { game: id, x: e.clientX, y: e.clientY }
  document.addEventListener('mousedown', closeMenu)
}

function closeMenu() {
  menu.value = null
  document.removeEventListener('mousedown', closeMenu)
}
onBeforeUnmount(closeMenu)

function hideGame(id: string) {
  closeMenu()
  void settings.update({ sidebarGames: settings.settings.sidebarGames.filter((g) => g !== id) })
}

const activeGame = computed(() => {
  if (route.name === 'game') return String(route.params.game)
  if (typeof route.params.id === 'string') return accounts.account(route.params.id)?.game
  return undefined
})
</script>

<template>
  <nav class="side" :class="{ collapsed }" :aria-label="t('nav.games')">
    <div class="brand">
      <AppLogo :size="30" />
      <template v-if="!collapsed">
        <span>{{ t('app.name') }}</span>
        <span class="ver" style="margin-left: auto">v{{ version }}</span>
      </template>
    </div>
    <button
      class="btn primary importbtn"
      :title="`${t('nav.import')}（Ctrl+I）`"
      data-test="open-import"
      @click="ui.openImport()"
    >
      <AppIcon name="download" />
      <span v-if="!collapsed">{{ t('nav.import') }}</span>
    </button>

    <RouterLink
      to="/"
      class="nav"
      :class="{ on: route.name === 'overview' }"
      :title="collapsed ? t('nav.overview') : undefined"
    >
      <span class="navico"><AppIcon name="grid" :size="15" /></span>
      <span v-if="!collapsed">{{ t('nav.overview') }}</span>
    </RouterLink>

    <div class="grp">
      <span v-if="!collapsed">{{ t('nav.games') }}</span>
    </div>
    <RouterLink
      v-for="g in shown"
      :key="g.id"
      :to="`/game/${g.id}`"
      class="nav"
      :class="{ on: activeGame === g.id }"
      :title="collapsed ? gameName(g.id) : undefined"
      :data-test="`nav-${g.id}`"
      @contextmenu="onContext($event, g.id)"
    >
      <GameIcon :game="g.id" :size="24" />
      <template v-if="!collapsed">
        <span class="name">{{ gameName(g.id) }}</span>
        <span v-if="counts.get(g.id)" class="cnt num">{{ counts.get(g.id) }}</span>
      </template>
    </RouterLink>

    <template v-if="hidden.length">
      <button
        class="nav dim"
        :title="collapsed ? t('nav.addGame') : undefined"
        :aria-expanded="adding"
        data-test="add-game"
        @click="adding = !adding"
      >
        <span class="navico add"><AppIcon name="plus" :size="12" /></span>
        <span v-if="!collapsed">{{ t('nav.addGame') }}</span>
      </button>
      <template v-if="adding">
        <button
          v-for="g in hidden"
          :key="g.id"
          class="nav sub"
          :data-test="`add-${g.id}`"
          :title="collapsed ? gameName(g.id) : undefined"
          @click="addGame(g.id)"
        >
          <GameIcon :game="g.id" :size="20" />
          <span v-if="!collapsed">{{ gameName(g.id) }}</span>
        </button>
      </template>
    </template>

    <div class="spacer" />
    <div class="iconrow" :class="{ vertical: collapsed }">
      <RouterLink
        to="/settings"
        class="iconbtn"
        :class="{ on: route.name === 'settings' }"
        :aria-label="t('nav.settings')"
        :title="`${t('nav.settings')}（Ctrl+,）`"
      >
        <AppIcon name="gear" :size="20" />
      </RouterLink>
      <button
        class="iconbtn"
        :aria-label="isDark() ? t('nav.toLight') : t('nav.toDark')"
        :title="isDark() ? t('nav.toLight') : t('nav.toDark')"
        data-test="toggle-theme"
        @click="toggleTheme"
      >
        <AppIcon :name="isDark() ? 'sun' : 'moon'" :size="20" />
      </button>
      <button
        class="iconbtn"
        :aria-label="collapsed ? t('nav.expand') : t('nav.collapse')"
        :title="collapsed ? t('nav.expand') : t('nav.collapse')"
        style="margin-left: auto"
        @click="emit('toggle')"
      >
        <AppIcon name="sidebar" :size="18" />
      </button>
    </div>

    <div v-if="menu" class="menu ctx" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }" @mousedown.stop>
      <button class="mi" @click="hideGame(menu.game)">{{ t('nav.hideFromSidebar') }}</button>
    </div>
  </nav>
</template>

<style scoped>
.side {
  flex: 0 0 240px;
  width: 240px;
  background: var(--surface);
  border-right: 1px solid var(--border);
  padding: 16px 12px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
}
.side.collapsed {
  flex-basis: 64px;
  width: 64px;
  padding: 16px 8px;
}
.brand {
  font-size: 17px;
  font-weight: 700;
  padding: 2px 6px 14px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.collapsed .brand {
  justify-content: center;
  padding: 2px 0 14px;
}
.importbtn {
  min-height: 38px;
  margin-bottom: 8px;
}
.collapsed .importbtn {
  padding: 0;
}
.nav {
  text-align: left;
  border: 0;
  background: transparent;
  color: var(--text);
  border-radius: 6px;
  padding: 0 10px;
  min-height: 36px;
  cursor: pointer;
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  text-decoration: none;
  font-size: 14px;
}
.collapsed .nav {
  justify-content: center;
  padding: 0;
}
.nav:hover {
  background: var(--hover);
}
.nav.on {
  background: var(--selected);
  color: var(--primary);
  font-weight: 600;
  box-shadow: inset 3px 0 0 var(--primary);
}
.nav.dim {
  color: var(--text2);
  font-size: 13px;
}
.nav.sub {
  font-size: 13px;
  padding-left: 18px;
}
.collapsed .nav.sub {
  padding-left: 0;
}
.name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.navico {
  width: 24px;
  height: 24px;
  border-radius: 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--track);
  color: var(--text2);
  flex: 0 0 24px;
}
.nav.on .navico {
  background: var(--primary);
  color: var(--onprimary);
}
.navico.add {
  background: transparent;
  border: 1.5px dashed var(--border);
}
.cnt {
  margin-left: auto;
  font-size: 11px;
  color: var(--text2);
  font-weight: 500;
  background: var(--track);
  border-radius: 999px;
  padding: 0 7px;
  line-height: 18px;
}
.grp {
  font-size: 11px;
  font-weight: 600;
  color: var(--text2);
  padding: 14px 10px 4px;
  margin-top: 8px;
  border-top: 1px solid var(--border);
  letter-spacing: 0.5px;
}
.iconrow {
  display: flex;
  gap: 4px;
  padding: 0 4px;
}
.iconrow.vertical {
  flex-direction: column;
  align-items: center;
  padding: 0;
}
.iconrow.vertical .iconbtn {
  margin-left: 0 !important;
}
.ctx {
  position: fixed;
  min-width: 160px;
}
</style>
