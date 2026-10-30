<script setup lang="ts">
import { computed } from 'vue'
import { fmtBytes } from '@/composables/format'
import { useNames } from '@/composables/useNames'
import { useUpdaterStore } from '@/stores/updater'

// 发现新版本（F-24）：版本号、更新说明，「稍后」或「立即更新」（下载完成后自动重启）。
const updater = useUpdaterStore()
const { t } = useNames()

const downloading = computed(() => updater.status === 'downloading')
const pct = computed(() =>
  updater.total ? `${Math.min(100, (updater.downloaded / updater.total) * 100)}%` : null,
)
function close() {
  if (!downloading.value) updater.prompt = false
}
</script>

<template>
  <div v-if="updater.prompt && updater.info" class="overlay" @click.self="close">
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      :aria-label="t('update.title')"
      data-test="update-dialog"
      style="max-width: 520px"
      @keydown.escape="close"
    >
      <h2 class="ct">{{ t('update.title') }}</h2>
      <div>
        {{ t('update.body', { version: updater.info.version, current: updater.info.currentVersion }) }}
      </div>
      <pre v-if="updater.info.notes" class="notes">{{ updater.info.notes }}</pre>
      <div v-if="downloading" class="col">
        <div class="muted">
          {{ t('update.downloading') }}
          <template v-if="updater.downloaded">
            {{ fmtBytes(updater.downloaded) }}<template v-if="updater.total"> / {{ fmtBytes(updater.total) }}</template>
          </template>
        </div>
        <div class="track"><div class="fill" :style="{ width: pct ?? '30%' }" /></div>
      </div>
      <div v-if="updater.status === 'error' && updater.error" class="notice error">
        {{ t('update.failed') }}
      </div>
      <div class="row" style="justify-content: flex-end">
        <button class="btn" :disabled="downloading" @click="close">{{ t('update.later') }}</button>
        <button class="btn primary" :disabled="downloading" data-test="update-install" @click="updater.install()">
          {{ t('update.install') }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.notes {
  margin: 0;
  max-height: 240px;
  overflow: auto;
  white-space: pre-wrap;
  font: inherit;
  font-size: 13px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 10px 12px;
}
.col {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
</style>
