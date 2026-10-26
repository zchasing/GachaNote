// 对 bindings.ts 的薄封装：命令失败时抛出的值统一成 ErrorDto，便于按错误码显示文案。
import { commands, events, type ErrorDto } from '@/bindings'

export { commands as api, events }

export type ApiError = ErrorDto

/** 把命令抛出的任意值转成 ErrorDto。 */
export function toApiError(e: unknown): ApiError {
  if (e && typeof e === 'object' && 'code' in e && typeof (e as ApiError).code === 'string') {
    const err = e as Partial<ApiError>
    return { code: err.code!, message: err.message ?? '', detail: err.detail ?? null }
  }
  const message = e instanceof Error ? e.message : String(e)
  return { code: 'internal', message, detail: message }
}

/** 是否运行在 Tauri 里（浏览器预览、单元测试未 mock 时为 false）。 */
export function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}
