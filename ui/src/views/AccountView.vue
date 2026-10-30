<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { api } from '@/api'
import AppIcon from '@/components/AppIcon.vue'
import ExportDialog from '@/components/ExportDialog.vue'
import GameIcon from '@/components/GameIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import ShareDialog from '@/components/ShareDialog.vue'
import { fmtInt } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useRelative } from '@/composables/useRelative'
import { useAccountsStore } from '@/stores/accounts'
import { useSettingsStore } from '@/stores/settings'
import { useUiStore } from '@/stores/ui'

// 账号详情（UI/UX 文档 3.3）：顶栏是游戏名 + 账号切换 + 更新记录，下行是「统计 / 记录 / 分析 / 时间线」标签。
const route = useRoute()
const router = useRouter()
const accounts = useAccountsStore()
const settings = useSettingsStore()
const ui = useUiStore()
const { report } = useErrors()
const { t, locale, gameName } = useNames()
const { rel, abs } = useRelative()

const id = computed(() => String(route.params.id))
const account = computed(() => accounts.account(id.value))
const siblings = computed(() => (account.value ? accounts.ofGame(account.value.game) : []))
const label = (a: { nickname: string | null; uid: string }) => a.nickname ?? a.uid

// 记住每款游戏上次查看的账号，边栏点游戏时直接进入
watch(
  account,
  (a) => {
    if (a && settings.settings.lastAccounts[a.game] !== a.id) {
      void settings.update({ lastAccounts: { ...settings.settings.lastAccounts, [a.game]: a.id } })
    }
  },
  { immediate: true },
)

const menu = ref<'accounts' | 'more' | null>(null)
function openMenu(which: 'accounts' | 'more') {
  menu.value = menu.value === which ? null : which
  if (menu.value) document.addEventListener('mousedown', closeMenu)
}
function closeMenu(e?: MouseEvent) {
  if (e && (e.target as HTMLElement).closest?.('.anchor')) return
  menu.value = null
  document.removeEventListener('mousedown', closeMenu)
}
onBeforeUnmount(() => document.removeEventListener('mousedown', closeMenu))

function switchTo(target: string) {
  menu.value = null
  const tab = ['records', 'analysis', 'timeline'].includes(String(route.name)) ? String(route.name) : 'stats'
  void router.push(`/account/${target}/${tab}`)
}

async function copyUid() {
  if (!account.value) return
  try {
    await navigator.clipboard.writeText(account.value.uid)
    ui.toast(t('common.copied'))
  } catch (e) {
    report(e)
  }
}

function update() {
  if (account.value) ui.openImport({ game: account.value.game })
}

function importNew() {
  menu.value = null
  if (account.value) ui.openImport({ game: account.value.game })
}

const exporting = ref(false)
const sharing = ref(false)
const shareStats = computed(() => accounts.stats[id.value])

async function openShare() {
  menu.value = null
  try {
    await accounts.loadStats(id.value)
    sharing.value = true
  } catch (e) {
    report(e)
  }
}
const renaming = ref(false)
const nickname = ref('')
const renameInput = ref<HTMLInputElement | null>(null)

async function startRename() {
  menu.value = null
  nickname.value = account.value?.nickname ?? ''
  renaming.value = true
  await nextTick()
  renameInput.value?.focus()
}

async function saveRename() {
  if (!account.value) return
  try {
    await api.renameAccount(account.value.id, nickname.value.trim() || null)
    renaming.value = false
    await accounts.load()
    ui.toast(t('account.renamed'))
  } catch (e) {
    report(e)
  }
}

async function remove() {
  menu.value = null
  const a = account.value
  if (!a) return
  const ok = await ui.confirm({
    title: t('account.deleteTitle'),
    body: t('account.deleteBody', { name: `${gameName(a.game)} · ${label(a)}` }),
    confirm: t('account.deleteConfirm', { n: fmtInt(a.records, locale.value) }),
    danger: true,
  })
  if (!ok) return
  try {
    await api.deleteAccount(a.id)
    const rest = { ...settings.settings.lastAccounts }
    delete rest[a.game]
    void settings.update({ lastAccounts: rest })
    await accounts.invalidate([a.id])
    ui.toast(t('account.deleted'))
    void router.replace(`/game/${a.game}`)
  } catch (e) {
    report(e)
  }
}
</script>

<template>
  <template v-if="account">
    <PageHeader>
      <template #icon><GameIcon :game="account.game" :size="32" /></template>
      <template #title>
        <div class="row" style="gap: 8px; flex-wrap: nowrap">
          <h1 class="h1">{{ gameName(account.game) }}</h1>
          <div class="anchor">
            <button
              class="btn small"
              :aria-expanded="menu === 'accounts'"
              data-test="account-switch"
              @click="openMenu('accounts')"
            >
              {{ label(account) }}<AppIcon name="chevronDown" :size="14" />
            </button>
            <div v-if="menu === 'accounts'" class="menu">
              <button
                v-for="a in siblings"
                :key="a.id"
                class="mi"
                :class="{ on: a.id === account.id }"
                @click="switchTo(a.id)"
              >
                <span>{{ label(a) }}</span>
                <span class="muted" style="margin-left: auto">{{ t('account.uid', { uid: a.uid }) }}</span>
              </button>
              <div class="msep" />
              <button class="mi" data-test="import-new" @click="importNew">
                <AppIcon name="plus" :size="14" />{{ t('account.importNew') }}
              </button>
            </div>
          </div>
        </div>
      </template>
      <template #subtitle>
        <button class="linkbtn uid" :title="t('account.copyUid')" @click="copyUid">
          {{ t('account.uid', { uid: account.uid }) }}
        </button>
        ·
        <span :title="abs(account.lastImportAt)">
          {{ t('account.lastUpdated', { time: rel(account.lastImportAt) }) }}
        </span>
      </template>
      <template #actions>
        <button class="btn primary" :title="`${t('account.update')}（Ctrl+R）`" data-test="update" @click="update">
          <AppIcon name="refresh" />{{ t('account.update') }}
        </button>
        <div class="anchor">
          <button
            class="iconbtn"
            :aria-label="t('common.more')"
            :aria-expanded="menu === 'more'"
            data-test="account-more"
            @click="openMenu('more')"
          >
            <AppIcon name="more" />
          </button>
          <div v-if="menu === 'more'" class="menu right">
            <button class="mi" @click="((menu = null), (exporting = true))">{{ t('common.export') }}</button>
            <button class="mi" data-test="open-share" @click="openShare">{{ t('account.share') }}</button>
            <button class="mi" @click="startRename">{{ t('common.rename') }}</button>
            <div class="msep" />
            <button class="mi danger" data-test="delete-account" @click="remove">{{ t('common.delete') }}</button>
          </div>
        </div>
      </template>
      <template #tabs>
        <nav class="tabs">
          <RouterLink :to="`/account/${account.id}/stats`" class="tab" active-class="on">
            {{ t('account.tabs.stats') }}
          </RouterLink>
          <RouterLink :to="`/account/${account.id}/records`" class="tab" active-class="on">
            {{ t('account.tabs.records') }}
          </RouterLink>
          <RouterLink :to="`/account/${account.id}/analysis`" class="tab" active-class="on" data-test="tab-analysis">
            {{ t('account.tabs.analysis') }}
          </RouterLink>
          <RouterLink :to="`/account/${account.id}/timeline`" class="tab" active-class="on" data-test="tab-timeline">
            {{ t('account.tabs.timeline') }}
          </RouterLink>
        </nav>
      </template>
    </PageHeader>
    <RouterView :key="account.id" />

    <ExportDialog v-if="exporting" :account-ids="[account.id]" @close="exporting = false" />
    <ShareDialog
      v-if="sharing && shareStats"
      :account="account"
      :stats="shareStats"
      :pool="ui.selectedPool[account.id]"
      @close="sharing = false"
    />
    <div v-if="renaming" class="overlay" @click.self="renaming = false">
      <form
        class="modal"
        role="dialog"
        aria-modal="true"
        :aria-label="t('account.renameTitle')"
        style="max-width: 440px"
        @submit.prevent="saveRename"
        @keydown.escape="renaming = false"
      >
        <h2 class="ct">{{ t('account.renameTitle') }}</h2>
        <input
          ref="renameInput"
          v-model="nickname"
          class="input"
          maxlength="40"
          :placeholder="t('account.renamePlaceholder')"
          data-test="rename-input"
        />
        <div class="row" style="justify-content: flex-end">
          <button type="button" class="btn" @click="renaming = false">{{ t('common.cancel') }}</button>
          <button type="submit" class="btn primary">{{ t('common.save') }}</button>
        </div>
      </form>
    </div>
  </template>
  <template v-else>
    <PageHeader :title="t('account.notFound')" />
    <div class="content">
      <div class="wrap">
        <div class="empty">
          <div>{{ t('account.notFound') }}</div>
          <RouterLink to="/" class="btn">{{ t('nav.overview') }}</RouterLink>
        </div>
      </div>
    </div>
  </template>
</template>

<style scoped>
.h1 {
  font-size: 22px;
  font-weight: 600;
  white-space: nowrap;
}
.uid {
  font-size: 12px;
  color: var(--text2);
  min-height: 0;
}
</style>
