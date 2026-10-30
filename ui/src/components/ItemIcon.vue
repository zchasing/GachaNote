<script setup lang="ts">
import { computed } from 'vue'
import { useIconsStore } from '@/stores/icons'

// 物品头像（T-63）：有头像时显示图片；没有或加载失败时，`slot` 模式保留同样大小的空位
// （出金历史的布局不变），`inline` 模式不占位，只剩文字。
const props = withDefaults(
  defineProps<{ game: string; name: string; itemId?: string | null; size?: number; mode?: 'slot' | 'inline' }>(),
  { itemId: null, size: 24, mode: 'inline' },
)
const icons = useIconsStore()
const src = computed(() => icons.url(props.game, props.name, props.itemId))
</script>

<template>
  <img
    v-if="src"
    class="iicon"
    :src="src"
    :width="size"
    :height="size"
    alt=""
    loading="lazy"
    data-test="item-icon"
    @error="icons.markBroken(src)"
  />
  <span v-else-if="mode === 'slot'" class="islot" :style="{ width: `${size}px`, height: `${size}px` }" />
</template>

<style scoped>
.iicon {
  border-radius: 6px;
  flex: 0 0 auto;
  object-fit: cover;
  background: var(--track);
  vertical-align: middle;
}
</style>
