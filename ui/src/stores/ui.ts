import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { ApiError } from '@/api'

export interface Toast {
  id: number
  text: string
  kind: 'info' | 'error'
  error?: ApiError
}

export interface ConfirmOptions {
  title: string
  body: string
  confirm: string
  danger?: boolean
}

export interface ImportRequest {
  /** 已知游戏时从第二步开始（「更新记录」「+ 导入新账号」、空游戏页）。 */
  game?: string
  /** 直接进入某种方式的准备步骤。 */
  method?: 'url' | 'file'
}

/** 界面状态：轻提示、确认框、导入弹窗、各账号选中的卡池。 */
export const useUiStore = defineStore('ui', () => {
  const toasts = ref<Toast[]>([])
  const confirmState = ref<(ConfirmOptions & { resolve: (ok: boolean) => void }) | null>(null)
  const importRequest = ref<ImportRequest | null>(null)
  /** 统计、记录标签共用的卡池选择：账号 ID → 卡池类型（`overview` 表示账号总览）。 */
  const selectedPool = ref<Record<string, string>>({})
  /** 时间线图表的选区，按「距今天几天」保存，切换卡池或账号后仍然生效；`null` 表示全部。 */
  const timelineRange = ref<{ from: number; to: number } | null>(null)
  let nextId = 0

  function toast(text: string, kind: Toast['kind'] = 'info', error?: ApiError) {
    const id = ++nextId
    toasts.value.push({ id, text, kind, error })
    // 错误提示带详情，停留更久
    setTimeout(() => dismiss(id), kind === 'error' ? 8000 : 3000)
  }

  function dismiss(id: number) {
    toasts.value = toasts.value.filter((t) => t.id !== id)
  }

  function confirm(opts: ConfirmOptions): Promise<boolean> {
    confirmState.value?.resolve(false)
    return new Promise((resolve) => {
      confirmState.value = { ...opts, resolve }
    })
  }

  function settleConfirm(ok: boolean) {
    confirmState.value?.resolve(ok)
    confirmState.value = null
  }

  function openImport(req: ImportRequest = {}) {
    importRequest.value = req
  }

  function closeImport() {
    importRequest.value = null
  }

  return {
    toasts,
    confirmState,
    importRequest,
    selectedPool,
    timelineRange,
    toast,
    dismiss,
    confirm,
    settleConfirm,
    openImport,
    closeImport,
  }
})
