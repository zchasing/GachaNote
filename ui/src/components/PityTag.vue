<script setup lang="ts">
import { useI18n } from 'vue-i18n'

// 大 / 小保底标签：只在有大小保底机制的卡池显示（UI/UX 文档 5.2）。
defineProps<{ guaranteed: boolean; uncertain?: boolean }>()
const { t } = useI18n()
</script>

<template>
  <span
    class="gtag"
    :class="guaranteed ? 'big' : 'small'"
    :title="uncertain ? t('common.inferred') : undefined"
    data-test="pity-tag"
  >
    <svg v-if="guaranteed" width="12" height="12" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 2l8 3v6c0 5-3.4 9.3-8 11-4.6-1.7-8-6-8-11V5z" fill="currentColor" />
      <path
        d="M8.2 12.2l2.6 2.6 5-5.2"
        fill="none"
        stroke="#FFF6DD"
        stroke-width="2.4"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
    <svg
      v-else
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.2"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M12 2l8 3v6c0 5-3.4 9.3-8 11-4.6-1.7-8-6-8-11V5z" />
    </svg>
    {{ guaranteed ? t('pity.guaranteed') : t('pity.fiftyFifty') }}<template v-if="uncertain">?</template>
  </span>
</template>

<style scoped>
.gtag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  font-weight: 600;
  line-height: 22px;
  height: 22px;
  padding: 0 9px 0 7px;
  border-radius: 999px;
  white-space: nowrap;
  letter-spacing: 0.5px;
}
.big {
  background: linear-gradient(135deg, #f7d27a 0%, #e2a73c 100%);
  color: #4a3000;
  box-shadow:
    0 1px 2px rgba(160, 110, 20, 0.35),
    inset 0 1px 0 rgba(255, 255, 255, 0.45);
}
.small {
  background: transparent;
  color: var(--text2);
  box-shadow: inset 0 0 0 1px var(--border);
}
</style>
