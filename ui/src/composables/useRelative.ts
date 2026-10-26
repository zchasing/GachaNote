import { useI18n } from 'vue-i18n'
import { fmtLocal, relative } from './format'

/** 「3 小时前」及悬停用的绝对时间。 */
export function useRelative() {
  const { t } = useI18n()
  return {
    rel(ms: number | null | undefined): string {
      if (ms === null || ms === undefined) return t('time.never')
      const r = relative(ms)
      return 'n' in r ? t(r.key, { n: r.n }) : t(r.key)
    },
    abs(ms: number | null | undefined): string | undefined {
      return ms === null || ms === undefined ? undefined : fmtLocal(ms)
    },
  }
}
