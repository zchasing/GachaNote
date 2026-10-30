<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useNames } from '@/composables/useNames'

// 时间线下方的日期选区（UI/UX 文档 3.6）：缩略图显示每天的抽数，选区外加遮罩。
// 两端手柄拉伸（最少 1 天），选区框整体平移；键盘方向键每次 1 天，两侧按钮每次平移半个选区。
// 选区为闭区间 [start, end]（天的下标）。
const props = defineProps<{ values: number[]; startLabel: string; endLabel: string }>()
const range = defineModel<[number, number]>({ required: true })
const { t } = useNames()

const n = computed(() => props.values.length)
const max = computed(() => Math.max(1, ...props.values))
const isAll = computed(() => range.value[0] === 0 && range.value[1] === n.value - 1)

/** 缩略图柱子的路径：每天一列，宽度按总天数等分（viewBox 宽度 = 天数）。 */
const miniPath = computed(() =>
  props.values
    .map((v, i) => (v > 0 ? `M${i + 0.1} 100V${100 - (v / max.value) * 100}h0.8V100z` : ''))
    .join(''),
)

const left = computed(() => (range.value[0] / n.value) * 100)
const right = computed(() => ((n.value - 1 - range.value[1]) / n.value) * 100)

function clamp(s: number, e: number): [number, number] {
  const last = n.value - 1
  s = Math.max(0, Math.min(s, last))
  e = Math.max(s, Math.min(e, last))
  return [s, e]
}

function move(by: number) {
  const [s, e] = range.value
  const len = e - s
  const ns = Math.max(0, Math.min(s + by, n.value - 1 - len))
  range.value = [ns, ns + len]
}

function pan(dir: -1 | 1) {
  const len = range.value[1] - range.value[0] + 1
  move(dir * Math.max(1, Math.floor(len / 2)))
}
const canLeft = computed(() => range.value[0] > 0)
const canRight = computed(() => range.value[1] < n.value - 1)

// ---------- 拖动 ----------

const box = ref<HTMLDivElement | null>(null)
let drag: { kind: 'start' | 'end' | 'move'; x: number; from: [number, number] } | null = null

function dayAt(dx: number) {
  const w = box.value?.getBoundingClientRect().width || 1
  return Math.round((dx / w) * n.value)
}

function down(kind: 'start' | 'end' | 'move', e: PointerEvent) {
  if (kind === 'move' && isAll.value) return
  e.preventDefault()
  drag = { kind, x: e.clientX, from: [...range.value] }
  window.addEventListener('pointermove', onMove)
  window.addEventListener('pointerup', up)
}

function onMove(e: PointerEvent) {
  if (!drag) return
  const d = dayAt(e.clientX - drag.x)
  const [s, e0] = drag.from
  if (drag.kind === 'start') range.value = clamp(Math.min(s + d, e0), e0)
  else if (drag.kind === 'end') range.value = clamp(s, Math.max(e0 + d, s))
  else {
    const len = e0 - s
    const ns = Math.max(0, Math.min(s + d, n.value - 1 - len))
    range.value = [ns, ns + len]
  }
}

function up() {
  drag = null
  window.removeEventListener('pointermove', onMove)
  window.removeEventListener('pointerup', up)
}
onBeforeUnmount(up)

// ---------- 键盘 ----------

function key(kind: 'start' | 'end' | 'move', e: KeyboardEvent) {
  const by = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : 0
  if (!by) return
  e.preventDefault()
  const [s, en] = range.value
  if (kind === 'start') range.value = clamp(Math.min(s + by, en), en)
  else if (kind === 'end') range.value = clamp(s, Math.max(en + by, s))
  else move(by)
}
</script>

<template>
  <div class="brushrow">
    <button
      class="panbtn"
      :disabled="!canLeft"
      :aria-label="t('timeline.panLeft')"
      data-test="pan-left"
      @click="pan(-1)"
    >
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
        <path d="M10 3.5L5.5 8 10 12.5" />
      </svg>
    </button>
    <div ref="box" class="brush" :aria-label="t('timeline.brush')" data-test="brush">
      <svg class="mini" :viewBox="`0 0 ${n} 100`" preserveAspectRatio="none" aria-hidden="true">
        <path :d="miniPath" />
      </svg>
      <div class="mask" :style="{ left: 0, width: `${left}%` }" />
      <div class="mask" :style="{ right: 0, width: `${right}%` }" />
      <div
        class="win"
        :class="{ fixed: isAll }"
        :style="{ left: `${left}%`, right: `${right}%` }"
        tabindex="0"
        role="slider"
        :aria-label="t('timeline.window')"
        :aria-valuetext="`${startLabel} – ${endLabel}`"
        data-test="brush-window"
        @pointerdown="down('move', $event)"
        @keydown="key('move', $event)"
      />
      <div
        class="handle"
        :style="{ left: `${left}%` }"
        tabindex="0"
        role="slider"
        :aria-label="t('timeline.startHandle')"
        :aria-valuetext="startLabel"
        data-test="brush-start"
        @pointerdown="down('start', $event)"
        @keydown="key('start', $event)"
      ><span /></div>
      <div
        class="handle"
        :style="{ left: `${100 - right}%` }"
        tabindex="0"
        role="slider"
        :aria-label="t('timeline.endHandle')"
        :aria-valuetext="endLabel"
        data-test="brush-end"
        @pointerdown="down('end', $event)"
        @keydown="key('end', $event)"
      ><span /></div>
    </div>
    <button
      class="panbtn"
      :disabled="!canRight"
      :aria-label="t('timeline.panRight')"
      data-test="pan-right"
      @click="pan(1)"
    >
      <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
        <path d="M6 3.5L10.5 8 6 12.5" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.brushrow {
  display: flex;
  align-items: center;
  gap: 6px;
}
.panbtn {
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text2);
  cursor: pointer;
  flex: 0 0 auto;
}
.panbtn:disabled {
  opacity: 0.4;
  cursor: default;
}
.brush {
  position: relative;
  flex: 1;
  height: 40px;
  border-radius: 6px;
  background: var(--bg);
  border: 1px solid var(--border);
  touch-action: none;
  user-select: none;
}
.mini {
  position: absolute;
  inset: 4px 0 0;
  width: 100%;
  height: calc(100% - 4px);
}
.mini path {
  fill: var(--text2);
  opacity: 0.45;
}
.mask {
  position: absolute;
  top: 0;
  bottom: 0;
  background: var(--track);
  opacity: 0.75;
}
.win {
  position: absolute;
  top: -1px;
  bottom: -1px;
  border: 2px solid var(--primary);
  border-radius: 6px;
  cursor: grab;
}
.win.fixed {
  cursor: default;
}
.win:focus-visible,
.handle:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
}
.handle {
  position: absolute;
  top: 50%;
  width: 12px;
  height: 26px;
  transform: translate(-50%, -50%);
  background: var(--surface);
  border: 2px solid var(--primary);
  border-radius: 4px;
  cursor: ew-resize;
  display: flex;
  align-items: center;
  justify-content: center;
}
.handle span {
  width: 2px;
  height: 10px;
  background: var(--primary);
  border-radius: 1px;
}
</style>
