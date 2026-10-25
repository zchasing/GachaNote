// 让 `pnpm tauri <命令>` 在 ui/ 目录下也能找到 ../src-tauri（tauri CLI 默认只在当前目录的子目录里查找）
import { fileURLToPath } from 'node:url'
import cli from '@tauri-apps/cli'

process.env.TAURI_APP_PATH ??= fileURLToPath(new URL('../../src-tauri', import.meta.url))

cli.run(process.argv.slice(2), 'pnpm tauri').catch((err) => {
  console.error(err.message)
  process.exit(1)
})
