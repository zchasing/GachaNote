<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useUiStore } from '@/stores/ui'

// 确认框：只用于删除账号、恢复备份、迁移数据目录等会丢数据的操作。
const ui = useUiStore()
const { t } = useI18n()
</script>

<template>
  <div v-if="ui.confirmState" class="overlay" @click.self="ui.settleConfirm(false)">
    <div
      class="modal"
      role="alertdialog"
      aria-modal="true"
      :aria-label="ui.confirmState.title"
      style="max-width: 440px"
      @keydown.escape="ui.settleConfirm(false)"
    >
      <h2 class="ct">{{ ui.confirmState.title }}</h2>
      <p style="margin: 0">{{ ui.confirmState.body }}</p>
      <div class="row" style="justify-content: flex-end">
        <button class="btn" @click="ui.settleConfirm(false)">{{ t('common.cancel') }}</button>
        <button
          class="btn"
          :class="ui.confirmState.danger ? 'danger' : 'primary'"
          data-test="confirm-ok"
          @click="ui.settleConfirm(true)"
        >
          {{ ui.confirmState.confirm }}
        </button>
      </div>
    </div>
  </div>
</template>
