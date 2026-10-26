import { createI18n } from 'vue-i18n'
import en from './en.json'
import zhCN from './zh-CN.json'

export type Locale = 'zh-CN' | 'en'

export const LOCALES: Locale[] = ['zh-CN', 'en']

export const i18n = createI18n({
  legacy: false,
  locale: 'zh-CN',
  fallbackLocale: 'zh-CN',
  messages: { 'zh-CN': zhCN, en },
  // 物品名、UID 等来自数据而不是语言包，缺失的键直接回退到传入的默认值
  missingWarn: false,
  fallbackWarn: false,
})

/** 设置里的语言（`system` / `zh-CN` / `en`）换算成实际使用的语言包。 */
export function resolveLocale(setting: string, system = navigator.language): Locale {
  if (setting === 'zh-CN' || setting === 'en') return setting
  return system.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en'
}

export function setLocale(locale: Locale) {
  i18n.global.locale.value = locale
  document.documentElement.lang = locale
}
