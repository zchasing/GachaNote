<script setup lang="ts">
import { save } from '@tauri-apps/plugin-dialog'
import { computed, ref } from 'vue'
import type { ExportFormat } from '@/bindings'
import { api } from '@/api'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useAccountsStore } from '@/stores/accounts'
import { useGamesStore } from '@/stores/games'
import { useUiStore } from '@/stores/ui'
import SegmentedControl from './SegmentedControl.vue'

// 导出弹窗：选格式后弹出系统保存框。UIGF 只能装米哈游三款游戏，含其他游戏时不提供。
const props = defineProps<{ accountIds: string[] }>()
const emit = defineEmits<{ close: [] }>()
const accounts = useAccountsStore()
const games = useGamesStore()
const ui = useUiStore()
const { report } = useErrors()
const { t, locale } = useNames()

const allHoyo = computed(() =>
  props.accountIds.every((id) => {
    const a = accounts.account(id)
    return a && games.game(a.game)?.vendor === 'hoyoverse'
  }),
)
const options = computed(() =>
  (['uigf', 'gnj', 'csv'] as ExportFormat[])
    .filter((f) => f !== 'uigf' || allHoyo.value)
    .map((f) => ({ value: f, label: t(`export.${f}`) })),
)
const format = ref<ExportFormat>(allHoyo.value ? 'uigf' : 'gnj')
const busy = ref(false)

const ext: Record<ExportFormat, string> = { uigf: 'json', gnj: 'json', csv: 'csv' }

function defaultName() {
  const d = new Date()
  const stamp = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}`
  const one = props.accountIds.length === 1 ? accounts.account(props.accountIds[0]!) : undefined
  const who = one ? `${one.game}-${one.uid}` : 'all'
  return `gachanote-${who}-${stamp}.${ext[format.value]}`
}

async function run() {
  busy.value = true
  try {
    const path = await save({
      defaultPath: defaultName(),
      filters: [{ name: t(`export.${format.value}`), extensions: [ext[format.value]] }],
    })
    if (!path) return
    const res = await api.exportAccounts(props.accountIds, format.value, path)
    ui.toast(t('settings.exported', { n: res.records.toLocaleString(locale.value) }))
    emit('close')
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      :aria-label="t('account.exportTitle')"
      style="max-width: 480px"
      data-test="export-dialog"
      @keydown.escape="emit('close')"
    >
      <h2 class="ct">{{ t('account.exportTitle') }}</h2>
      <div class="field">
        {{ t('export.format') }}
        <SegmentedControl v-model="format" :options="options" :label="t('export.format')" />
      </div>
      <div v-if="!allHoyo" class="muted">{{ t('export.uigfOnlyHoyo') }}</div>
      <div class="row" style="justify-content: flex-end">
        <button class="btn" @click="emit('close')">{{ t('common.cancel') }}</button>
        <button class="btn primary" :disabled="busy" data-test="export-run" @click="run">
          {{ t('export.choose') }}
        </button>
      </div>
    </div>
  </div>
</template>
