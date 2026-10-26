<script setup lang="ts">
import { open } from '@tauri-apps/plugin-dialog'
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import type { AppInfo, BackupDto, MetaInfo } from '@/bindings'
import { api, inTauri } from '@/api'
import AppIcon from '@/components/AppIcon.vue'
import ExportDialog from '@/components/ExportDialog.vue'
import GameIcon from '@/components/GameIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import SegmentedControl from '@/components/SegmentedControl.vue'
import { fmtBytes, fmtInt, fmtLocal } from '@/composables/format'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useSettingsStore } from '@/stores/settings'
import { useUiStore } from '@/stores/ui'

// 设置（UI/UX 文档 3.6）：通用、数据、卡池数据包、凭证、关于。
const route = useRoute()
const settings = useSettingsStore()
const accounts = useAccountsStore()
const ui = useUiStore()
const { report } = useErrors()
const { t, locale, gameName } = useNames()

const SECTIONS = ['general', 'data', 'metadata', 'credentials', 'about'] as const
type Section = (typeof SECTIONS)[number]
const section = computed<Section>(() => {
  const s = String(route.params.section ?? '')
  return (SECTIONS as readonly string[]).includes(s) ? (s as Section) : 'general'
})

const info = ref<AppInfo | null>(null)
const backups = ref<BackupDto[]>([])
const meta = ref<MetaInfo | null>(null)
const proxy = ref(settings.settings.proxy ?? '')
const needsRestart = ref(false)
const checking = ref(false)
const exportIds = ref<string[] | null>(null)

async function refresh() {
  if (!inTauri()) return
  try {
    const [i, b, m] = await Promise.all([api.appInfo(), api.listBackups(), api.metaInfo()])
    info.value = i
    backups.value = b
    meta.value = m
  } catch (e) {
    report(e)
  }
}
onMounted(refresh)
watch(
  () => accounts.version,
  () => void refresh(),
)

// ── 通用 ──
const language = computed({
  get: () => settings.settings.language,
  set: (v: string) => void settings.update({ language: v }),
})
const theme = computed({
  get: () => settings.settings.theme,
  set: (v: string) => void settings.update({ theme: v }),
})

async function saveProxy() {
  const v = proxy.value.trim() || null
  if (v === settings.settings.proxy) return
  try {
    await settings.update({ proxy: v })
    needsRestart.value = true
    ui.toast(t('settings.proxySaved'))
  } catch (e) {
    report(e)
  }
}

// ── 数据 ──
async function changeDir(reset = false) {
  try {
    let dir: string | null
    if (reset) dir = info.value?.defaultDataDir ?? null
    else {
      const picked = await open({ directory: true, multiple: false })
      dir = typeof picked === 'string' ? picked : null
    }
    if (!dir) return
    await api.setDataDir(dir)
    needsRestart.value = true
    ui.toast(t('settings.dataDirMoved'))
    await refresh()
  } catch (e) {
    report(e)
  }
}

async function backupNow() {
  try {
    await api.createBackup()
    ui.toast(t('settings.backupDone'))
    await refresh()
  } catch (e) {
    report(e)
  }
}

async function restore(b: BackupDto) {
  const ok = await ui.confirm({
    title: t('settings.restoreTitle'),
    body: t('settings.restoreBody', { name: b.name }),
    confirm: t('settings.restoreConfirm'),
    danger: true,
  })
  if (!ok) return
  try {
    await api.restoreBackup(b.name)
    ui.toast(t('settings.restored'))
    await refresh()
  } catch (e) {
    report(e)
  }
}

async function removeAccount(id: string) {
  const a = accounts.account(id)
  if (!a) return
  const ok = await ui.confirm({
    title: t('account.deleteTitle'),
    body: t('account.deleteBody', { name: `${gameName(a.game)} · ${a.nickname ?? a.uid}` }),
    confirm: t('account.deleteConfirm', { n: fmtInt(a.records, locale.value) }),
    danger: true,
  })
  if (!ok) return
  try {
    await api.deleteAccount(id)
    await accounts.invalidate([id])
    ui.toast(t('account.deleted'))
  } catch (e) {
    report(e)
  }
}

// ── 卡池数据包 ──
async function checkMeta() {
  checking.value = true
  try {
    const r = await api.updateMetadata()
    if (r.outcome === 'updated') ui.toast(t('settings.metaUpdated', { version: r.version }))
    else if (r.outcome === 'appTooOld')
      ui.toast(t('settings.metaAppTooOld', { version: r.version, min: r.minAppVersion ?? '?' }), 'error')
    else ui.toast(t('settings.metaUpToDate', { version: r.version }))
    meta.value = await api.metaInfo()
  } catch (e) {
    report(e)
  } finally {
    checking.value = false
  }
}

function openDir(kind: 'data' | 'backups' | 'logs') {
  api.openDir(kind).catch(report)
}

function restart() {
  api.restartApp().catch(report)
}

const version = __APP_VERSION__
</script>

<template>
  <PageHeader :title="t('settings.title')">
    <template #icon><AppIcon name="gear" :size="24" /></template>
    <template #actions>
      <button v-if="needsRestart" class="btn primary" data-test="restart" @click="restart">
        {{ t('common.restart') }}
      </button>
    </template>
    <template #tabs>
      <nav class="tabs">
        <RouterLink
          v-for="s in SECTIONS"
          :key="s"
          :to="`/settings/${s}`"
          class="tab"
          :class="{ on: section === s }"
          :data-test="`section-${s}`"
        >
          {{ t(`settings.sections.${s}`) }}
        </RouterLink>
      </nav>
    </template>
  </PageHeader>
  <div class="content">
    <div class="wrap narrow">
      <!-- 通用 -->
      <template v-if="section === 'general'">
        <div class="card">
          <div class="srow">
            <div>{{ t('settings.language') }}</div>
            <SegmentedControl
              v-model="language"
              :label="t('settings.language')"
              :options="[
                { value: 'system', label: t('settings.languageSystem') },
                { value: 'zh-CN', label: '简体中文' },
                { value: 'en', label: 'English' },
              ]"
            />
          </div>
          <div class="srow">
            <div>{{ t('settings.theme') }}</div>
            <SegmentedControl
              v-model="theme"
              :label="t('settings.theme')"
              :options="[
                { value: 'system', label: t('settings.themes.system') },
                { value: 'light', label: t('settings.themes.light') },
                { value: 'dark', label: t('settings.themes.dark') },
              ]"
            />
          </div>
          <div class="srow">
            <div>
              {{ t('settings.proxy') }}
              <div class="muted">{{ t('settings.proxyHint') }}</div>
            </div>
            <input
              v-model="proxy"
              class="input"
              style="width: 260px"
              :placeholder="t('settings.proxyPlaceholder')"
              data-test="proxy"
              @blur="saveProxy"
              @keydown.enter="saveProxy"
            />
          </div>
        </div>
      </template>

      <!-- 数据 -->
      <template v-else-if="section === 'data'">
        <div class="card">
          <div class="ct">{{ t('settings.dataDir') }}</div>
          <div class="muted">{{ t('settings.dataDirHint') }}</div>
          <code class="path" data-test="data-dir">{{ info?.dataDir ?? '—' }}</code>
          <div v-if="info" class="muted">{{ t('settings.dbSize', { size: fmtBytes(info.dbSize) }) }}</div>
          <div class="row">
            <button class="btn small" @click="changeDir()">{{ t('settings.change') }}</button>
            <button
              v-if="info && info.dataDir !== info.defaultDataDir"
              class="btn small"
              @click="changeDir(true)"
            >
              {{ t('settings.resetDir') }}
            </button>
            <button class="btn small" @click="openDir('data')">
              <AppIcon name="folder" :size="14" />{{ t('settings.openDir') }}
            </button>
          </div>
        </div>

        <div class="card">
          <div class="row" style="justify-content: space-between">
            <div>
              <div class="ct">{{ t('settings.backups') }}</div>
              <div class="muted">{{ t('settings.backupsHint') }}</div>
            </div>
            <button class="btn small" data-test="backup-now" @click="backupNow">{{ t('settings.backupNow') }}</button>
          </div>
          <div v-if="backups.length === 0" class="muted">{{ t('settings.noBackups') }}</div>
          <div v-for="b in backups" :key="b.name" class="lrow" data-test="backup-row">
            <span class="num">{{ fmtLocal(b.createdAt) }}</span>
            <span class="muted">{{ b.name }}</span>
            <span class="muted num" style="margin-left: auto">{{ fmtBytes(b.size) }}</span>
            <button class="btn small" @click="restore(b)">{{ t('settings.restore') }}</button>
          </div>
        </div>

        <div class="card">
          <div class="row" style="justify-content: space-between">
            <div>
              <div class="ct">{{ t('settings.accounts') }}</div>
              <div class="muted">{{ t('settings.exportHint') }}</div>
            </div>
            <button
              class="btn small"
              :disabled="accounts.accounts.length === 0"
              data-test="export-all"
              @click="exportIds = accounts.accounts.map((a) => a.id)"
            >
              {{ t('settings.exportAll') }}
            </button>
          </div>
          <div v-if="accounts.accounts.length === 0" class="muted">{{ t('settings.noAccounts') }}</div>
          <div v-for="a in accounts.accounts" :key="a.id" class="lrow" data-test="settings-account">
            <GameIcon :game="a.game" :size="20" />
            <span>{{ gameName(a.game) }} · {{ a.nickname ?? a.uid }}</span>
            <span class="muted num">{{ t('common.count', { n: fmtInt(a.records, locale) }) }}</span>
            <span style="margin-left: auto" />
            <button class="btn small" @click="exportIds = [a.id]">{{ t('common.export') }}</button>
            <button class="btn small" @click="removeAccount(a.id)">{{ t('common.delete') }}</button>
          </div>
        </div>
      </template>

      <!-- 卡池数据包 -->
      <template v-else-if="section === 'metadata'">
        <div class="card">
          <div class="muted">{{ t('settings.metaHint') }}</div>
          <div class="srow">
            <div>{{ t('settings.metaVersion') }}</div>
            <div>
              <b class="num" data-test="meta-version">{{ meta?.version ?? '—' }}</b>
              <span v-if="meta" class="muted">
                · {{ t(`settings.metaOrigin.${meta.origin === 'cache' ? 'cache' : 'bundled'}`) }}
              </span>
            </div>
          </div>
          <div class="row">
            <button class="btn small" :disabled="checking" data-test="meta-check" @click="checkMeta">
              <AppIcon name="refresh" :size="14" />{{ checking ? t('settings.metaChecking') : t('settings.metaCheck') }}
            </button>
            <label class="check">
              <input
                type="checkbox"
                :checked="settings.settings.metaAutoUpdate"
                @change="settings.update({ metaAutoUpdate: ($event.target as HTMLInputElement).checked })"
              />
              {{ t('settings.metaAuto') }}
            </label>
          </div>
        </div>
      </template>

      <!-- 凭证 -->
      <template v-else-if="section === 'credentials'">
        <div class="empty">{{ t('settings.credentialsEmpty') }}</div>
      </template>

      <!-- 关于 -->
      <template v-else>
        <div class="card">
          <div class="srow">
            <div>{{ t('settings.version') }}</div>
            <b class="num">v{{ info?.version ?? version }}</b>
          </div>
          <div class="srow">
            <div>
              {{ t('settings.logs') }}
              <div class="muted path">{{ info?.logDir ?? '—' }}</div>
            </div>
            <button class="btn small" @click="openDir('logs')">{{ t('settings.openLogs') }}</button>
          </div>
          <div class="muted">{{ t('settings.license') }}</div>
          <div class="muted">{{ t('settings.privacy') }}</div>
          <a href="https://github.com/zchasing/GachaNote" target="_blank" rel="noreferrer">{{ t('settings.source') }}</a>
        </div>
      </template>
    </div>
  </div>
  <ExportDialog v-if="exportIds" :account-ids="exportIds" @close="exportIds = null" />
</template>

<style scoped>
.narrow {
  max-width: 820px;
}
.srow {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
  min-height: 40px;
}
.srow + .srow {
  border-top: 1px solid var(--border);
  padding-top: 12px;
}
.lrow {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  border-top: 1px solid var(--border);
  padding-top: 10px;
}
.path {
  font-family: var(--mono, ui-monospace, monospace);
  font-size: 12px;
  word-break: break-all;
}
code.path {
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 8px;
}
</style>
