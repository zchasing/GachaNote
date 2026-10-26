import { watchEffect, onScopeDispose, ref } from 'vue'
import { useSettingsStore } from '@/stores/settings'

/** 按设置切换 `<html data-theme>`；「跟随系统」时监听 prefers-color-scheme。 */
export function useTheme() {
  const settings = useSettingsStore()
  const media = typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null
  const systemDark = ref(media?.matches ?? false)
  const onChange = (e: MediaQueryListEvent) => (systemDark.value = e.matches)
  media?.addEventListener('change', onChange)
  onScopeDispose(() => media?.removeEventListener('change', onChange))

  const isDark = () =>
    settings.settings.theme === 'dark' || (settings.settings.theme === 'system' && systemDark.value)

  watchEffect(() => {
    document.documentElement.dataset.theme = isDark() ? 'dark' : 'light'
  })

  /** 边栏快捷切换：在浅色与深色之间切换（不再跟随系统）。 */
  function toggle() {
    return settings.update({ theme: isDark() ? 'light' : 'dark' })
  }

  return { isDark, toggle }
}
