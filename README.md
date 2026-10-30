# GachaNote

个人抽卡记录与数据分析工具。把六款游戏的抽卡记录导入到本地，一眼看清每个账号的当前保底、平均出 UP 抽数和小保底歪率。

[English](#english) · [使用说明](docs/user-guide.md) · [隐私说明](docs/privacy.md) · [下载](https://github.com/zchasing/GachaNote/releases)

## 功能（v0.2）

- **六款游戏**：原神、崩坏：星穹铁道、绝区零、鸣潮、明日方舟、明日方舟：终末地；同一游戏可以有多个账号。
- **导入**：米哈游三款与鸣潮可直接从游戏里拉取（自动查找抽卡链接，也可粘贴）；明日方舟、终末地登录鹰角账号即可拉取（也可粘贴 token，可加密记住 token 一键更新）；所有游戏都可导入文件（UIGF v2–v4、SRGF、GNJ、CSV）。重复导入自动去重，写入前自动备份。
- **统计**：各卡池当前保底与距保底抽数、平均出 UP 抽数、小保底歪率、出金历史、稀有度构成；估算的数字会标注。
- **分析**：下一抽 / 未来 N 抽的出金与出 UP 概率曲线、欧非评价（和同样抽数的玩家比）、出金抽数分布、月度趋势、成就徽章。
- **总览**：一屏看所有游戏、所有账号的保底与总抽数。
- **记录列表**：按卡池、稀有度、时间、名称筛选，高星显示当次抽数与是否 UP。
- **导出与备份**：米哈游三款导出 UIGF v4.2，其余导出 GNJ，全部可导出 CSV；数据库自动保留最近 5 份备份。
- **其他**：中文 / English，浅色 / 深色 / 跟随系统；应用内检查并安装新版本。

后续版本计划加入卡池时间线和分享图（v0.3）。完整规划见 [开发计划](docs/05-dev-plan.md)。

## 安装

1. 到 [Releases](https://github.com/zchasing/GachaNote/releases) 下载 `GachaNote_<版本>_x64-setup.exe`。
2. 双击安装。安装到当前用户，不需要管理员权限；系统缺少 WebView2 时安装程序会自动下载。
3. 安装包暂时没有代码签名，Windows 可能弹出「Windows 已保护你的电脑」：点 **更多信息 → 仍要运行**。详见 [使用说明 · 安装](docs/user-guide.md#安装)，也可以先 [核对 SHA-256](docs/user-guide.md#核对安装包)。

目前只支持 Windows 10 / 11（64 位）。手机端在后续版本开发。

## 隐私

所有数据只保存在你的电脑上，不收集任何使用数据，没有账号系统和服务器。应用只会访问游戏官方的抽卡记录接口（你主动拉取时）、本仓库的卡池数据包和 GitHub Releases。详见 [隐私说明](docs/privacy.md)。

## 开发

技术栈：Rust + Tauri 2 + Vue 3。本地开发、提交规范与 CI 见 [CONTRIBUTING.md](CONTRIBUTING.md)，设计文档见 [docs/](docs/README.md)。

## 致谢

- 米哈游抽卡接口与卡池数据参考 [HoYo.Gacha](https://github.com/lgou2w/HoYo.Gacha)（MIT / Apache-2.0），卡池数据的来源说明见 [metadata/NOTICE.md](metadata/NOTICE.md)。
- 文件格式遵循 [UIGF](https://uigf.org/) 标准。

GachaNote 是非官方的个人项目，与米哈游、库洛、鹰角网络没有关联。游戏名称与相关内容的权利归各自所有者。

## 许可证

[MIT](LICENSE)

---

## English

GachaNote is a personal gacha history tracker and analyzer for Windows. It imports pull records for Genshin Impact, Honkai: Star Rail, Zenless Zone Zero, Wuthering Waves, Arknights and Arknights: Endfield, and shows the current pity, average pulls per featured item and 50/50 loss rate for every account.

- **Import**: fetch directly from the miHoYo games and Wuthering Waves (the gacha URL is found automatically, or paste it), sign in to a Hypergryph account to fetch Arknights and Endfield (or paste a token; it can be remembered, encrypted with Windows DPAPI), or import UIGF v2–v4, SRGF, GNJ or CSV files. Duplicates are skipped and the database is backed up before every write.
- **Stats**: pity per banner, average pulls per featured item, 50/50 loss rate, top-rarity history and a cross-game overview. Estimated values are marked.
- **Analysis**: odds of a top-rarity or featured pull within the next N pulls, a luck rating against players with the same number of pulls, pull distribution, monthly trend and achievements.
- **Export**: UIGF v4.2 for the miHoYo games, GNJ for the others, CSV for all.
- **Privacy**: everything stays on your computer. No telemetry, no account, no server. The app can check GitHub Releases for updates (can be turned off). See [docs/privacy.md](docs/privacy.md) (Chinese).

**Install**: download the `.exe` from [Releases](https://github.com/zchasing/GachaNote/releases) and run it (per-user install, no admin rights). The installer is not code-signed yet, so Windows SmartScreen may warn you: click **More info → Run anyway**. SHA-256 checksums are listed on each release page.

The user guide is in Chinese: [docs/user-guide.md](docs/user-guide.md). The app itself is available in English (Settings → General → Language).

GachaNote is an unofficial fan project and is not affiliated with miHoYo/HoYoverse, Kuro Games or Hypergryph. Licensed under [MIT](LICENSE).
