<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef, watchEffect } from 'vue'
import type { EChartsCoreOption, ECharts } from 'echarts/core'
import type { CssVar } from '@/composables/chart'

// 图表容器：动态加载 ECharts，`option` 是以 CSS 变量为参数的函数，主题切换时重新计算。
const props = withDefaults(
  defineProps<{
    option: (css: CssVar) => EChartsCoreOption
    label: string
    height?: number
  }>(),
  { height: 260 },
)

const el = ref<HTMLDivElement | null>(null)
const chart = shallowRef<ECharts | null>(null)
/** `<html data-theme>` 每变化一次加 1，触发重新计算配色。 */
const themeTick = ref(0)

const css: CssVar = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim()

let resize: ResizeObserver | null = null
let themeObserver: MutationObserver | null = null

onMounted(async () => {
  const { init } = await import('./echarts')
  if (!el.value) return
  chart.value = init(el.value, undefined, { renderer: 'svg' })
  if (typeof ResizeObserver === 'function') {
    resize = new ResizeObserver(() => chart.value?.resize())
    resize.observe(el.value)
  }
  themeObserver = new MutationObserver(() => (themeTick.value += 1))
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
})

watchEffect(() => {
  void themeTick.value
  const c = chart.value
  if (c) c.setOption(props.option(css), true)
})

onBeforeUnmount(() => {
  resize?.disconnect()
  themeObserver?.disconnect()
  chart.value?.dispose()
})
</script>

<template>
  <div ref="el" class="echart" role="img" :aria-label="label" :style="{ height: `${height}px` }" />
</template>

<style scoped>
.echart {
  width: 100%;
}
</style>
