//! 游戏插件接口（架构文档第 3 节）。
//!
//! 每款游戏实现一个 [`GamePlugin`]：规则模板（必须）、卡池类型映射、可选的拉取器。
//! 文件格式转换器 [`Converter`] 跨游戏通用（一个 UIGF v4 文件可含三款游戏），
//! 单独注册，不挂在某个插件下。

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::exchange::{ExportAccount, ImportedAccount};
use crate::ids::{AccountId, FormatId, GameId, PoolTypeId};
use crate::meta::{Classified, GameMeta};
use crate::model::{Cursor, NewRecord};
use crate::redact::{Redacted, Url};
use crate::rules::PoolRule;
use crate::task::{CancelToken, Progress};

/// 厂商。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    /// 米哈游。
    Hoyoverse,
    /// 鹰角网络。
    Hypergryph,
    /// 库洛游戏。
    Kuro,
    /// 其他。
    Other,
}

/// 在线拉取方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FetchKind {
    /// 抽卡记录链接。
    Url,
    /// 账号 token。
    AccountToken,
}

/// 游戏信息（`list_games` 返回给前端）。名称由前端 i18n 按 `id` 查找。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInfo {
    /// 游戏。
    pub id: GameId,
    /// 厂商。
    pub vendor: Vendor,
    /// 印象色（`#RRGGBB`）。
    pub theme_color: String,
    /// 卡池类型，按界面显示顺序。
    pub pool_types: Vec<PoolTypeId>,
    /// 支持的在线拉取方式。
    pub fetch: Option<FetchKind>,
    /// 支持导入的文件格式。
    pub formats: Vec<FormatId>,
}

/// 拉取来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchSource {
    /// 游戏。
    pub game: GameId,
    /// 抽卡记录链接（URL 拉取）。
    pub url: Option<Redacted<String, Url>>,
    /// 已保存凭据所属的账号（token 拉取）。
    pub account: Option<AccountId>,
}

/// 增量拉取的起点：每个卡池类型最新的一条记录。
pub type Since = BTreeMap<PoolTypeId, Cursor>;

/// 平台相关路径（游戏安装目录、日志位置），由外壳注入，便于在 Linux CI 上测试。
pub trait PathProvider: Send + Sync {
    /// 某款游戏可能的安装目录或日志文件，按优先级排列。
    fn game_paths(&self, game: &GameId) -> Vec<PathBuf>;
}

/// 在线拉取器。
#[async_trait]
pub trait Fetcher: Send + Sync {
    /// 拉取方式。
    fn kind(&self) -> FetchKind;
    /// 自动查找链接或已保存凭据。
    async fn discover(&self, env: &dyn PathProvider) -> Result<Vec<FetchSource>>;
    /// 拉取记录；`since` 为空时全量拉取。
    async fn fetch(
        &self,
        src: &FetchSource,
        since: &Since,
        progress: &dyn Progress,
        cancel: &CancelToken,
    ) -> Result<Vec<ImportedAccount>>;
}

/// 文件格式转换器。
pub trait Converter: Send + Sync {
    /// 格式。
    fn format(&self) -> FormatId;
    /// 根据文件开头若干字节判断是否为本格式。
    fn sniff(&self, head: &[u8]) -> bool;
    /// 解码。文件结构不合法时整体报错；个别记录不合法时放进 [`ImportedAccount::failures`]。
    fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>>;
    /// 编码。不支持导出的格式返回 [`crate::AppError::Invalid`]。
    fn encode(&self, data: &[ExportAccount], out: &mut dyn Write) -> Result<()>;
}

/// 一款游戏的插件。
pub trait GamePlugin: Send + Sync {
    /// 游戏 ID。
    fn id(&self) -> GameId;
    /// 游戏信息。
    fn info(&self) -> GameInfo;
    /// 每种卡池类型的规则模板（可按生效日期分段）。
    fn rules(&self) -> &[PoolRule];
    /// 把官方卡池类型代码映射为本软件的卡池类型；不认识时返回 `None`，该记录进入失败列表。
    fn pool_type(&self, official: &str) -> Option<PoolTypeId>;
    /// UID 是否符合本游戏格式。
    fn validate_uid(&self, uid: &str) -> bool {
        !uid.is_empty() && uid.chars().all(|c| c.is_ascii_alphanumeric())
    }
    /// 在线拉取器。
    fn fetcher(&self) -> Option<&dyn Fetcher> {
        None
    }
    /// 判断一只最高星是否 UP。默认按元数据包判断，游戏有特殊口径时覆盖。
    fn classify(&self, rec: &NewRecord, meta: &GameMeta) -> Classified {
        meta.classify(rec)
    }
}

/// 已注册的插件与转换器，由外壳在启动时组装。
#[derive(Default, Clone)]
pub struct PluginRegistry {
    games: Vec<Arc<dyn GamePlugin>>,
    converters: Vec<Arc<dyn Converter>>,
}

impl PluginRegistry {
    /// 空注册表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一款游戏；同 ID 的旧插件被替换。
    pub fn register_game(&mut self, plugin: Arc<dyn GamePlugin>) -> &mut Self {
        let id = plugin.id();
        self.games.retain(|p| p.id() != id);
        self.games.push(plugin);
        self
    }

    /// 注册一个转换器；同格式的旧转换器被替换。
    pub fn register_converter(&mut self, converter: Arc<dyn Converter>) -> &mut Self {
        let f = converter.format();
        self.converters.retain(|c| c.format() != f);
        self.converters.push(converter);
        self
    }

    /// 按 ID 取插件。
    pub fn game(&self, id: &GameId) -> Option<&Arc<dyn GamePlugin>> {
        self.games.iter().find(|p| &p.id() == id)
    }

    /// 全部插件，按注册顺序。
    pub fn games(&self) -> &[Arc<dyn GamePlugin>] {
        &self.games
    }

    /// 按格式取转换器。
    pub fn converter(&self, format: FormatId) -> Option<&Arc<dyn Converter>> {
        self.converters.iter().find(|c| c.format() == format)
    }

    /// 根据文件开头识别格式：返回第一个认领的转换器。
    pub fn sniff(&self, head: &[u8]) -> Option<&Arc<dyn Converter>> {
        self.converters.iter().find(|c| c.sniff(head))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! 测试用的最小插件：卡池类型代码原样映射。

    use super::*;
    use crate::rules::{PityCurve, PityScope, UpRule};

    pub struct TestGame {
        pub id: &'static str,
        pub rules: Vec<PoolRule>,
    }

    impl TestGame {
        pub fn genshin() -> Self {
            let curve = PityCurve {
                base: 0.006,
                soft_start: Some(74),
                step: 0.06,
                hard: 90,
            };
            let rule = |pool: &str, up| PoolRule {
                pool_type: pool.into(),
                since: None,
                rarity_top: 5,
                curve,
                up,
                extras: vec![],
                pity_scope: PityScope::SharedByType,
            };
            Self {
                id: "genshin",
                rules: vec![
                    rule(
                        "character",
                        UpRule::FiftyFifty {
                            rate: 0.5,
                            guarantee_next: true,
                        },
                    ),
                    rule("standard", UpRule::None),
                ],
            }
        }
    }

    impl GamePlugin for TestGame {
        fn id(&self) -> GameId {
            self.id.into()
        }
        fn info(&self) -> GameInfo {
            GameInfo {
                id: self.id(),
                vendor: Vendor::Hoyoverse,
                theme_color: "#C8A464".into(),
                pool_types: self.rules.iter().map(|r| r.pool_type.clone()).collect(),
                fetch: None,
                formats: vec![FormatId::UigfV4],
            }
        }
        fn rules(&self) -> &[PoolRule] {
            &self.rules
        }
        fn pool_type(&self, official: &str) -> Option<PoolTypeId> {
            match official {
                "301" | "400" => Some("character".into()),
                "200" => Some("standard".into()),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::TestGame;
    use super::*;

    struct Fake(FormatId, &'static [u8]);

    impl Converter for Fake {
        fn format(&self) -> FormatId {
            self.0
        }
        fn sniff(&self, head: &[u8]) -> bool {
            head.starts_with(self.1)
        }
        fn decode(&self, _input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
            Ok(vec![])
        }
        fn encode(&self, _data: &[ExportAccount], _out: &mut dyn Write) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn registry_lookup_and_replace() {
        let mut reg = PluginRegistry::new();
        reg.register_game(Arc::new(TestGame::genshin()))
            .register_game(Arc::new(TestGame::genshin()))
            .register_converter(Arc::new(Fake(FormatId::Gnj, b"{")))
            .register_converter(Arc::new(Fake(FormatId::GnCsv, b"game,")))
            .register_converter(Arc::new(Fake(FormatId::Gnj, b"[")));
        assert_eq!(reg.games().len(), 1);
        let g = reg.game(&"genshin".into()).unwrap();
        assert_eq!(g.info().pool_types.len(), 2);
        assert!(g.fetcher().is_none());
        assert!(g.validate_uid("100000001"));
        assert!(!g.validate_uid(""));
        assert!(!g.validate_uid("12 3"));
        assert!(reg.game(&"wuwa".into()).is_none());
        assert_eq!(reg.sniff(b"game,uid").unwrap().format(), FormatId::GnCsv);
        assert_eq!(reg.sniff(b"[1]").unwrap().format(), FormatId::Gnj);
        assert!(reg.sniff(b"{").is_none());
        assert!(reg.converter(FormatId::UigfV4).is_none());
        assert!(reg.converter(FormatId::Gnj).is_some());
    }
}
