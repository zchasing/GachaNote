<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import DropdownSelect from './DropdownSelect.vue'

// 分页：「共 N 条 · 第 x / y 页」、页码按钮与每页条数（UI/UX 文档 3.4）。
const props = defineProps<{ total: number; sizes?: number[] }>()
const page = defineModel<number>('page', { required: true })
const size = defineModel<number>('size', { required: true })
const { t, locale } = useI18n()

const pages = computed(() => Math.max(1, Math.ceil(props.total / size.value)))

/** 页码按钮：首尾页 + 当前页附近，中间用省略号。 */
const buttons = computed<(number | null)[]>(() => {
  const n = pages.value
  const cur = page.value
  const set = new Set([0, n - 1, cur - 1, cur, cur + 1].filter((p) => p >= 0 && p < n))
  const sorted = [...set].sort((a, b) => a - b)
  const out: (number | null)[] = []
  sorted.forEach((p, i) => {
    if (i > 0 && p - sorted[i - 1]! > 1) out.push(null)
    out.push(p)
  })
  return out
})

const sizeOptions = computed(() =>
  (props.sizes ?? [20, 50, 100]).map((n) => ({ value: n, label: t('records.perPage', { n }) })),
)

function setSize(v: number | number[]) {
  size.value = v as number
  page.value = 0
}
</script>

<template>
  <div class="pager">
    <span class="muted num" data-test="pager-summary">{{
      t('records.pager', {
        total: total.toLocaleString(locale),
        page: page + 1,
        pages,
      })
    }}</span>
    <div class="row" style="gap: 6px">
      <button class="pbtn" :disabled="page === 0" :aria-label="t('records.prev')" @click="page -= 1">‹</button>
      <template v-for="(p, i) in buttons" :key="i">
        <span v-if="p === null" class="muted">…</span>
        <button v-else class="pbtn num" :class="{ on: p === page }" :aria-current="p === page" @click="page = p">
          {{ p + 1 }}
        </button>
      </template>
      <button
        class="pbtn"
        :disabled="page >= pages - 1"
        :aria-label="t('records.next')"
        @click="page += 1"
      >
        ›
      </button>
      <DropdownSelect :model-value="size" :options="sizeOptions" @update:model-value="setSize" />
    </div>
  </div>
</template>

<style scoped>
.pager {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.pbtn {
  font-size: 13px;
  min-width: 32px;
  height: 32px;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  cursor: pointer;
}
.pbtn:hover:not(:disabled) {
  background: var(--hover);
}
.pbtn.on {
  background: var(--primary);
  border-color: var(--primary);
  color: var(--onprimary);
  font-weight: 600;
}
.pbtn:disabled {
  opacity: 0.4;
  cursor: default;
}
</style>
