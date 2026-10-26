<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useUiStore } from '@/stores/ui'

// 右下角轻提示：小操作 3 秒后消失；错误提示可展开技术详情。
const ui = useUiStore()
const { t } = useI18n()
const open = ref<number | null>(null)
</script>

<template>
  <div class="toasts" aria-live="polite">
    <div v-for="toast in ui.toasts" :key="toast.id" class="toast" :class="toast.kind" data-test="toast">
      <div class="row" style="flex-wrap: nowrap; align-items: flex-start">
        <span style="flex: 1">{{ toast.text }}</span>
        <button class="x" :aria-label="t('common.close')" @click="ui.dismiss(toast.id)">×</button>
      </div>
      <template v-if="toast.error?.detail">
        <button class="linkbtn" @click="open = open === toast.id ? null : toast.id">{{ t('common.details') }}</button>
        <code v-if="open === toast.id" class="detail">{{ toast.error.detail }}</code>
      </template>
    </div>
  </div>
</template>

<style scoped>
.toasts {
  position: fixed;
  right: 24px;
  bottom: 24px;
  z-index: 60;
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 420px;
}
.toast {
  background: var(--text);
  color: var(--surface);
  border-radius: 6px;
  padding: 8px 14px;
  font-size: 13px;
  box-shadow: var(--shadow);
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.toast.error {
  background: var(--lossbg);
  color: var(--loss);
  border: 1px solid var(--loss);
}
.toast .linkbtn {
  color: inherit;
  text-decoration: underline;
  align-self: flex-start;
}
.x {
  border: 0;
  background: transparent;
  color: inherit;
  cursor: pointer;
  font-size: 16px;
  line-height: 1;
  padding: 0 0 0 8px;
}
.detail {
  font-size: 12px;
  word-break: break-all;
  white-space: pre-wrap;
}
</style>
