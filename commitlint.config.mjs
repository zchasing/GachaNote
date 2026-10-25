// 提交信息规范：Conventional Commits，格式 <type>(<scope>): <描述>
// 说明见 CONTRIBUTING.md
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'type-enum': [2, 'always', [
      'feat', 'fix', 'docs', 'style', 'refactor', 'perf',
      'test', 'build', 'ci', 'chore', 'revert',
    ]],
    // scope 可省略；填写时建议使用以下取值，填其他值只给警告
    'scope-enum': [1, 'always', [
      'core', 'store', 'formats', 'games', 'meta', 'app', 'ui', 'metadata',
      'genshin', 'starrail', 'zzz', 'arknights', 'endfield', 'wuwa',
      'ci', 'docs', 'deps', 'release',
    ]],
    // 中文描述不区分大小写，关闭英文大小写规则
    'subject-case': [0],
    'header-max-length': [2, 'always', 100],
    'body-max-line-length': [0],
    'footer-max-line-length': [0],
  },
};
