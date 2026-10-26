<script setup lang="ts" generic="T extends string">
// 分段控件：浅灰槽 + 选中白底卡片，可在右侧带一个小胶囊（如卡池当前保底数）。
export interface Segment<V> {
  value: V
  label: string
  badge?: string | number | null
}

defineProps<{ options: Segment<T>[]; label?: string }>()
const model = defineModel<T>({ required: true })
</script>

<template>
  <div class="seg" role="tablist" :aria-label="label">
    <button
      v-for="o in options"
      :key="o.value"
      class="segb"
      :class="{ on: o.value === model }"
      role="tab"
      :aria-selected="o.value === model"
      :data-test="`seg-${o.value}`"
      @click="model = o.value"
    >
      {{ o.label }}<span v-if="o.badge !== undefined && o.badge !== null" class="pn num">{{ o.badge }}</span>
    </button>
  </div>
</template>

<style scoped>
.seg {
  display: inline-flex;
  align-self: flex-start;
  gap: 2px;
  padding: 3px;
  background: var(--track);
  border-radius: 8px;
  flex-wrap: wrap;
}
.segb {
  font-size: 13px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text2);
  padding: 0 12px;
  min-height: 32px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  transition:
    background var(--dur),
    color var(--dur);
}
.segb:hover {
  color: var(--text);
}
.segb.on {
  background: var(--surface);
  color: var(--text);
  font-weight: 600;
  box-shadow:
    0 1px 2px rgba(15, 15, 20, 0.12),
    0 0 0 1px var(--border);
}
.pn {
  font-weight: 600;
  font-size: 12px;
  min-width: 20px;
  text-align: center;
  padding: 0 6px;
  line-height: 20px;
  border-radius: 999px;
  background: var(--surface);
  color: var(--text2);
}
.segb.on .pn {
  background: var(--selected);
  color: var(--primary);
}
</style>
