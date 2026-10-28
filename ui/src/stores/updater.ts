import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { UpdateInfo } from '@/bindings'
import { api, events, inTauri, toApiError, type ApiError } from '@/api'

export type UpdateStatus = 'idle' | 'checking' | 'upToDate' | 'available' | 'downloading' | 'error'

/** 软件更新（F-24）：检查、提示、下载进度。安装完成后应用会自动重启。 */
export const useUpdaterStore = defineStore('updater', () => {
  const status = ref<UpdateStatus>('idle')
  const info = ref<UpdateInfo | null>(null)
  const error = ref<ApiError | null>(null)
  /** 是否显示「发现新版本」弹窗。 */
  const prompt = ref(false)
  const downloaded = ref(0)
  const total = ref<number | null>(null)

  /** 检查更新。`silent` 时（启动自动检查）失败不提示，只有发现新版本才弹窗。 */
  async function check(silent = false) {
    if (!inTauri() || status.value === 'checking' || status.value === 'downloading') return
    status.value = 'checking'
    error.value = null
    try {
      info.value = await api.checkUpdate()
      status.value = info.value ? 'available' : 'upToDate'
      if (info.value) prompt.value = true
    } catch (e) {
      error.value = toApiError(e)
      status.value = silent ? 'idle' : 'error'
    }
  }

  async function install() {
    status.value = 'downloading'
    downloaded.value = 0
    total.value = null
    const unlisten = await events.updateProgress.listen(({ payload }) => {
      downloaded.value = payload.downloaded
      total.value = payload.total
    })
    try {
      await api.installUpdate()
    } catch (e) {
      error.value = toApiError(e)
      status.value = 'error'
    } finally {
      unlisten()
    }
  }

  /** 自动更新还没配置签名公钥（发布前的版本）。 */
  const notConfigured = () => error.value?.code === 'invalid' && /not configured/.test(error.value.detail ?? error.value.message)

  return { status, info, error, prompt, downloaded, total, check, install, notConfigured }
})
