import { defineStore } from 'pinia'
import { ref } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { api, inTauri } from '@/api'

/**
 * 物品头像（T-63）：每款游戏一份「物品 ID / 名称 → 图标哈希」索引，按需加载；
 * 图片由后端的 `gnicon` 协议提供（下载、校验、缓存都在后端）。数据包更新后清空重载。
 */
export const useIconsStore = defineStore('icons', () => {
  const index = ref<Record<string, Record<string, string>>>({})
  /** 加载失败的图片，不再请求，直接显示文字。 */
  const broken = ref<Set<string>>(new Set())
  const loading = new Set<string>()

  function ensure(game: string) {
    if (!inTauri() || index.value[game] || loading.has(game)) return
    loading.add(game)
    api
      .getItemIcons(game)
      .then((map) => (index.value = { ...index.value, [game]: map as Record<string, string> }))
      .catch(() => (index.value = { ...index.value, [game]: {} }))
      .finally(() => loading.delete(game))
  }

  /** 物品头像地址；没有头像或已加载失败时为 `null`。先按物品 ID 找，再按名称。 */
  function url(game: string, name: string, id?: string | null): string | null {
    ensure(game)
    const map = index.value[game]
    const hash = (id ? map?.[id] : undefined) ?? map?.[name]
    if (!hash || broken.value.has(hash)) return null
    return convertFileSrc(`${game}/${hash}`, 'gnicon')
  }

  function markBroken(src: string) {
    const hash = src.split(/%2F|\//i).pop()
    if (hash) broken.value = new Set([...broken.value, hash])
  }

  function reset() {
    index.value = {}
    broken.value = new Set()
  }

  return { index, url, markBroken, reset }
})
