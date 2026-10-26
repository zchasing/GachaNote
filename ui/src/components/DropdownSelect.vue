<script setup lang="ts" generic="T extends string | number">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import AppIcon from './AppIcon.vue'

// 自绘下拉框（UI/UX 文档第 6 节）：单选选中后收起；多选保持展开，全选或不选时显示「全部」。
export interface Option<V> {
  value: V
  label: string
}

const props = defineProps<{ options: Option<T>[]; multiple?: boolean; label?: string }>()
const model = defineModel<T | T[]>({ required: true })
const { t } = useI18n()
const open = ref(false)
const root = ref<HTMLElement | null>(null)

const selected = computed<T[]>(() =>
  Array.isArray(model.value) ? model.value : [model.value as T],
)

const display = computed(() => {
  if (props.multiple) {
    const sel = selected.value
    if (sel.length === 0 || sel.length === props.options.length) return t('common.all')
    return props.options
      .filter((o) => sel.includes(o.value))
      .map((o) => o.label)
      .join('、')
  }
  return props.options.find((o) => o.value === model.value)?.label ?? ''
})

function pick(v: T) {
  if (props.multiple) {
    const sel = selected.value
    model.value = sel.includes(v) ? sel.filter((x) => x !== v) : [...sel, v]
  } else {
    model.value = v
    open.value = false
  }
}

function onDoc(e: MouseEvent) {
  if (root.value && !root.value.contains(e.target as Node)) open.value = false
}
function toggle() {
  open.value = !open.value
  if (open.value) document.addEventListener('mousedown', onDoc)
  else document.removeEventListener('mousedown', onDoc)
}
onBeforeUnmount(() => document.removeEventListener('mousedown', onDoc))
</script>

<template>
  <div ref="root" class="anchor">
    <button
      class="select"
      :class="{ open }"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-label="label"
      @click="toggle"
      @keydown.escape="open = false"
    >
      <span class="label">{{ display }}</span>
      <AppIcon name="chevronDown" :size="14" class="chev" />
    </button>
    <div v-if="open" class="menu" role="listbox" :aria-multiselectable="multiple" style="min-width: 100%">
      <button
        v-for="o in options"
        :key="o.value"
        class="mi"
        :class="{ on: selected.includes(o.value) }"
        role="option"
        :aria-selected="selected.includes(o.value)"
        @click="pick(o.value)"
      >
        <span class="ck"><AppIcon v-if="selected.includes(o.value)" name="check" :size="14" /></span>
        {{ o.label }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.select {
  font-size: 13px;
  min-height: 34px;
  min-width: 150px;
  padding: 0 10px 0 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.select:hover {
  border-color: var(--text2);
}
.select.open {
  border-color: var(--primary);
  box-shadow: 0 0 0 3px var(--selected);
}
.label {
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.chev {
  color: var(--text2);
  transition: transform var(--dur);
}
.select.open .chev {
  transform: rotate(180deg);
}
.ck {
  width: 16px;
  flex: 0 0 16px;
  display: inline-flex;
  color: var(--primary);
}
</style>
