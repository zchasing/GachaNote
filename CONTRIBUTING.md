# 贡献指南

## 分支与合并

- `main` 始终保持可构建、CI 全绿。
- 每个任务一个分支，命名为 `<type>/<任务编号>-<简述>`，例如 `feat/T-15-stats-engine`、`fix/T-21-authkey-expired`。
- 通过 PR 合并，统一使用 **squash merge**，PR 标题即最终的提交信息。
- 任务在 GitHub Projects 看板上跟踪，每个任务编号一张卡片；只有缺陷或需要讨论时才单独开 Issue。

## 提交信息规范

采用 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/)：

```text
<type>(<scope>): <描述>

<正文，可选>

<脚注，可选，例如 BREAKING CHANGE: ...>
```

| type | 用途 |
| --- | --- |
| `feat` | 新功能 |
| `fix` | 缺陷修复 |
| `docs` | 只改文档 |
| `style` | 格式调整，不影响逻辑 |
| `refactor` | 重构，既不是新功能也不是修复 |
| `perf` | 性能优化 |
| `test` | 新增或修改测试 |
| `build` | 构建系统、依赖 |
| `ci` | CI 配置 |
| `chore` | 其他杂项 |
| `revert` | 回滚 |

scope 可省略，建议取值：`core` `store` `formats` `games` `meta` `app` `ui` `metadata`、
游戏名 `genshin` `starrail` `zzz` `arknights` `endfield` `wuwa`，以及 `ci` `docs` `deps` `release`。

示例：

```text
feat(core): 新增保底状态机
fix(wuwa): 修复 Epic 安装路径下找不到 Client.log
docs: 补充架构文档 ER 图
```

描述可用中文，首字母不要大写，整行不超过 100 个字符。PR 标题和 PR 中的每条提交都会在 CI 中检查。

## CI 工作流

| 工作流 | 触发时机 | 内容 |
| --- | --- | --- |
| `CI`（ci.yml） | 每个 PR、推送到 `main` | 按改动路径只运行相关检查，见下表；最后由「CI 通过」汇总 |
| `PR 规范`（pr.yml） | PR 创建、编辑、推送 | PR 标题与提交信息格式；按路径自动打 `area/*`、`game/*` 标签 |
| `依赖安全`（security.yml） | 依赖清单变化、每周一 | cargo-deny（漏洞、许可证、来源）、pnpm audit |
| `Release`（release.yml） | 推送 `v*` 标签；手动触发 | 构建 Windows NSIS 安装包；标签发布时创建 Release 草稿和自动更新清单 |

`CI` 中各检查的触发条件：

| 检查 | 改动路径 | 内容 |
| --- | --- | --- |
| Rust 格式与 Clippy | `crates/**`、`src-tauri/**`、`Cargo.*` | `cargo fmt --check`、`cargo clippy -D warnings` |
| Rust 测试 | 同上 | `cargo test`，Linux 与 Windows 各跑一次 |
| 覆盖率门槛 | 同上 | `core`、`formats`、`store` 行覆盖率 ≥ 90%（cargo-llvm-cov） |
| 前后端类型绑定一致性 | `crates/**`、`src-tauri/**`、`ui/src/bindings.ts` | 重新生成 `bindings.ts`，必须与已提交版本一致 |
| 前端检查 | `ui/**` | `lint`、`type-check`、`test`、`build`（脚本存在才运行） |
| 卡池元数据包校验 | `metadata/**` | JSON 可解析、Schema 校验、`manifest.json` 中的 SHA-256 与文件一致 |
| 文档检查 | `**/*.md` | markdownlint |
| 工作流语法检查 | `.github/workflows/**` | actionlint |

改动 `ci.yml`、`.github/actions/` 或 `.github/path-filters.yml` 时，所有检查都会重跑。
对应目录还不存在时，相关检查会自动跳过。

### 约定

- `src-tauri` 的 crate 名为 `gachanote-app`，其中名为 `export_bindings` 的测试负责写出 `ui/src/bindings.ts`。
- `ui/package.json` 需声明 `packageManager`（如 `"pnpm@9.15.0"`），并提供 `lint`、`type-check`、`test`、`build` 脚本。
- 修改 `metadata/` 后运行 `python scripts/check_metadata.py --fix` 更新 `manifest.json` 的哈希。
- 发布前把 `src-tauri/tauri.conf.json` 的 `version` 改成与标签一致（标签 `v0.1.0` 对应 `0.1.0`），否则发布会失败。

### 仓库设置（一次性）

1. **分支保护**：Settings → Branches → 为 `main` 添加规则，勾选「Require status checks to pass」，
   必选检查只需要「CI 通过」「PR 标题」「提交信息」三项。
2. **合并方式**：Settings → General → Pull Requests，只保留「Allow squash merging」，
   默认提交信息选「Pull request title」。
3. **自动更新签名**（v0.2 起）：用 `pnpm tauri signer generate` 生成密钥，
   把私钥和密码分别存入仓库 Secrets `TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
