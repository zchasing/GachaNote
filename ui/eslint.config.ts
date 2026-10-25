import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import pluginVue from 'eslint-plugin-vue'
import { globalIgnores } from 'eslint/config'

export default defineConfigWithVueTs(
  {
    name: 'app/files-to-lint',
    files: ['**/*.{ts,mts,tsx,vue}'],
  },
  // bindings.ts 由 tauri-specta 生成，禁止手改，也不做 lint
  globalIgnores(['**/dist/**', '**/coverage/**', 'src/bindings.ts']),
  pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,
)
