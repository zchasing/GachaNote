<script setup lang="ts">
import { save } from '@tauri-apps/plugin-dialog'
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import type { AccountDto, AccountStatsDto, UpMark } from '@/bindings'
import { api } from '@/api'
import { fmtDate, maskUid } from '@/composables/format'
import { poolCards } from '@/composables/poolCards'
import { canvasPngBase64, drawShare, inkOn, PALETTES, type ShareContent } from '@/composables/shareImage'
import { useErrors } from '@/composables/useErrors'
import { useNames } from '@/composables/useNames'
import { useGamesStore } from '@/stores/games'
import { useUiStore } from '@/stores/ui'
import SegmentedControl from './SegmentedControl.vue'

// 分享图弹窗（UI/UX 文档 3.7，F-20）：左侧预览，右侧选项；导出 1080 px 宽的 PNG。
const props = defineProps<{ account: AccountDto; stats: AccountStatsDto; pool?: string }>()
const emit = defineEmits<{ close: [] }>()
const games = useGamesStore()
const ui = useUiStore()
const { report } = useErrors()
const { t, locale, gameName, gameMono, poolName, poolShort, rarityName } = useNames()

const RECENT = 10

const withPulls = computed(() => props.stats.pools.filter((p) => p.totalPulls > 0))
const pool = ref(
  withPulls.value.some((p) => p.poolType === props.pool) ? props.pool! : (withPulls.value[0]?.poolType ?? ''),
)
const pools = computed(() => withPulls.value.map((p) => ({ value: p.poolType, label: poolShort(props.account.game, p.poolType) })))

const hideUid = ref(true)
const theme = ref<'light' | 'dark'>(document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light')
const themes = computed(() => [
  { value: 'light' as const, label: t('share.light') },
  { value: 'dark' as const, label: t('share.dark') },
])

const game = computed(() => props.account.game)
const topName = computed(() => rarityName(game.value, games.game(game.value)?.rarityTop ?? 5))

function tag(up: UpMark | null): ShareContent['rows'][number]['tag'] {
  if (up === 'up') return { text: t('stats.up'), kind: 'up' }
  if (up === 'notUp') return { text: t('stats.lost'), kind: 'loss' }
  if (up === 'unknown') return { text: t('stats.unknown'), kind: 'neutral' }
  return null
}

const content = computed<ShareContent | null>(() => {
  const p = withPulls.value.find((x) => x.poolType === pool.value)
  if (!p) return null
  const info = games.pool(game.value, p.poolType)
  const cards = poolCards(p, info, t, locale.value)
  const guarantee = info?.guarantee ? (p.pity.guaranteed ? t('pity.guaranteed') : t('pity.fiftyFifty')) : null
  const color = games.game(game.value)?.themeColor ?? '#3b6fe0'
  const uid = hideUid.value ? maskUid(props.account.uid) : props.account.uid
  const name = props.account.nickname ?? (hideUid.value ? maskUid(props.account.uid) : props.account.uid)
  return {
    mono: gameMono(game.value),
    color,
    ink: inkOn(color),
    title: `${gameName(game.value)} · ${name}`,
    subtitle: `UID ${uid} · ${poolName(game.value, p.poolType)}`,
    metrics: [
      {
        label: t('stats.currentPity'),
        value: `${p.pity.sinceTop} / ${info?.hard ?? '?'}`,
        sub: guarantee ?? t('stats.remaining', { n: p.pity.remainingToHard }),
      },
      { label: cards.second.label, value: cards.second.value, sub: cards.second.sub },
      { label: cards.third.label, value: cards.third.value, sub: cards.third.sub },
    ],
    listTitle: t('share.recent', { n: RECENT, name: topName.value }),
    rows: [...p.tops]
      .reverse()
      .slice(0, RECENT)
      .map((top) => ({
        name: top.itemName,
        pulls: t('common.pulls', { n: top.pulls }),
        tag: info?.up === 'fiftyFifty' || info?.up === 'fixed' ? tag(top.up) : null,
      })),
    empty: t('share.noTops', { name: topName.value }),
    date: fmtDate(Date.now(), props.account.tzOffsetMin),
    app: 'GachaNote',
  }
})

/** 给读屏用的文字版，也便于测试检查内容。 */
const summary = computed(() => {
  const c = content.value
  if (!c) return ''
  return [c.title, c.subtitle, ...c.metrics.map((m) => `${m.label} ${m.value}`), ...c.rows.map((r) => `${r.name} ${r.pulls}`)].join('，')
})

const canvas = ref<HTMLCanvasElement | null>(null)
function render() {
  if (!canvas.value || !content.value) return
  const font = getComputedStyle(document.documentElement).getPropertyValue('--font').trim() || 'sans-serif'
  drawShare(canvas.value, content.value, PALETTES[theme.value], font)
}
onMounted(render)
watch([content, theme], () => void nextTick(render))

const busy = ref(false)

async function savePng() {
  if (!canvas.value) return
  busy.value = true
  try {
    render()
    const data = canvasPngBase64(canvas.value)
    const d = new Date()
    const stamp = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, '0')}${String(d.getDate()).padStart(2, '0')}`
    const path = await save({
      defaultPath: `gachanote-${game.value}-${pool.value}-${stamp}.png`,
      filters: [{ name: t('share.fileName'), extensions: ['png'] }],
    })
    if (!path) return
    await api.saveImage(path, data)
    ui.toast(t('share.saved'))
  } catch (e) {
    report(e)
  } finally {
    busy.value = false
  }
}

async function copyPng() {
  const c = canvas.value
  if (!c) return
  busy.value = true
  try {
    render()
    const blob = await new Promise<Blob | null>((resolve) => c.toBlob(resolve, 'image/png'))
    if (!blob || typeof ClipboardItem === 'undefined') throw new Error('clipboard unavailable')
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })])
    ui.toast(t('share.copied'))
  } catch {
    ui.toast(t('share.copyFailed'), 'error')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div
      class="modal wide"
      role="dialog"
      aria-modal="true"
      :aria-label="t('share.title')"
      data-test="share-dialog"
      @keydown.escape="emit('close')"
    >
      <div class="row" style="justify-content: space-between">
        <h2 class="ct" style="font-size: 17px">{{ t('share.title') }}</h2>
        <button class="btn small" data-test="share-close" @click="emit('close')">{{ t('common.close') }}</button>
      </div>
      <div class="share">
        <div class="shot">
          <canvas ref="canvas" role="img" :aria-label="summary" data-test="share-canvas" />
        </div>
        <div class="opts">
          <label class="check">
            <input v-model="hideUid" type="checkbox" data-test="share-hide-uid" />
            {{ t('share.hideUid') }}
          </label>
          <div class="field">
            {{ t('share.background') }}
            <SegmentedControl v-model="theme" :options="themes" :label="t('share.background')" />
          </div>
          <div class="field">
            {{ t('share.pool') }}
            <SegmentedControl v-model="pool" :options="pools" :label="t('share.pool')" />
          </div>
          <div class="muted">{{ t('share.size') }}</div>
          <div style="flex: 1" />
          <div class="row" style="justify-content: flex-end">
            <button class="btn" :disabled="busy" data-test="share-copy" @click="copyPng">{{ t('share.copy') }}</button>
            <button class="btn primary" :disabled="busy" data-test="share-save" @click="savePng">
              {{ t('share.save') }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.wide {
  max-width: 880px;
  width: calc(100vw - 48px);
}
.share {
  display: grid;
  grid-template-columns: minmax(0, 1.4fr) minmax(220px, 1fr);
  gap: 20px;
}
.shot {
  max-height: calc(100vh - 200px);
  overflow: auto;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--bg);
}
.shot canvas {
  display: block;
  width: 100%;
  height: auto;
}
.opts {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
</style>
