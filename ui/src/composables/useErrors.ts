import { useI18n } from 'vue-i18n'
import { toApiError, type ApiError } from '@/api'
import { useUiStore } from '@/stores/ui'

/** 把错误码换成「发生了什么 + 怎么做」的文案（UI/UX 文档第 6 节）。 */
export function useErrors() {
  const { t, te } = useI18n()
  const ui = useUiStore()

  function describe(e: unknown): { title: string; hint: string; error: ApiError } {
    const error = toApiError(e)
    const key = te(`errors.${error.code}.title`) ? error.code : 'unknown'
    return { title: t(`errors.${key}.title`), hint: t(`errors.${key}.hint`), error }
  }

  /** 显示错误提示（右下角，可展开详情）。 */
  function report(e: unknown) {
    const d = describe(e)
    ui.toast(`${d.title}。${d.hint}`, 'error', d.error)
  }

  return { describe, report }
}
