import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { Settings } from '@/bindings'
import { api, inTauri } from '@/api'

export const DEFAULT_SETTINGS: Settings = {
  language: 'system',
  theme: 'system',
  proxy: null,
  sidebarGames: [],
  lastAccounts: {},
  metaAutoUpdate: true,
}

/** 用户设置：保存在后端数据库，界面改动后立即写回。 */
export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<Settings>({ ...DEFAULT_SETTINGS })
  const loaded = ref(false)

  async function load() {
    if (inTauri()) {
      settings.value = { ...DEFAULT_SETTINGS, ...(await api.getSettings()) }
    }
    loaded.value = true
  }

  async function update(patch: Partial<Settings>) {
    settings.value = { ...settings.value, ...patch }
    if (inTauri()) await api.setSettings(settings.value)
  }

  return { settings, loaded, load, update }
})
