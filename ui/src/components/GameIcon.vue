<script setup lang="ts">
import { computed } from 'vue'
import { useGamesStore } from '@/stores/games'
import { useNames } from '@/composables/useNames'

// 游戏图标：官方图标随卡池数据包下载（T-60），未下载时显示主题色底 + 游戏首字的占位。
const props = withDefaults(defineProps<{ game: string; size?: number }>(), { size: 24 })
const games = useGamesStore()
const { gameMono, gameName } = useNames()

const color = computed(() => games.game(props.game)?.themeColor ?? '#888888')

/** 按主题色亮度选深色或白色字，保证对比度。 */
const ink = computed(() => {
  const hex = color.value.replace('#', '')
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
  const lum = 0.2126 * r! + 0.7152 * g! + 0.0722 * b!
  return lum > 0.5 ? '#1A1A12' : '#FFFFFF'
})
</script>

<template>
  <span
    class="gicon"
    :style="{
      width: `${size}px`,
      height: `${size}px`,
      background: color,
      color: ink,
      fontSize: `${Math.round(size * 0.52)}px`,
      borderRadius: `${Math.round(size / 4)}px`,
    }"
    role="img"
    :aria-label="gameName(game)"
    data-test="game-icon"
    >{{ gameMono(game) }}</span
  >
</template>

<style scoped>
.gicon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
  flex: 0 0 auto;
  line-height: 1;
}
</style>
