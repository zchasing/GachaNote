//! 领域模型：账号、抽卡记录、导入批次与查询。
//!
//! 时间一律为 UTC 毫秒（`i64`），展示时再按账号的 `tz_offset_min` 换算。

use serde::{Deserialize, Serialize};

use crate::ids::{AccountId, BatchId, FormatId, GameId, PoolTypeId, RecordId};

/// 中国服务器默认时区偏移（UTC+8），单位分钟。
pub const DEFAULT_TZ_OFFSET_MIN: i32 = 480;

/// 一个游戏账号。`(game, uid)` 唯一。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// 主键。
    pub id: AccountId,
    /// 所属游戏。
    pub game: GameId,
    /// 游戏内 UID。
    pub uid: String,
    /// 服务器：`cn` / `global` 或游戏自己的服务器代码。
    pub region: Option<String>,
    /// 备注名。
    pub nickname: Option<String>,
    /// 服务器时区偏移（分钟），展示时间用。
    pub tz_offset_min: i32,
    /// 创建时间（UTC 毫秒）。
    pub created_at: i64,
}

/// 新建或更新账号的参数。已存在同一 `(game, uid)` 时只补全为空的字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewAccount {
    /// 所属游戏。
    pub game: GameId,
    /// 游戏内 UID。
    pub uid: String,
    /// 服务器。
    pub region: Option<String>,
    /// 备注名。
    pub nickname: Option<String>,
    /// 服务器时区偏移（分钟）。
    pub tz_offset_min: i32,
}

impl NewAccount {
    /// 只有游戏与 UID、时区取默认值的账号。
    pub fn new(game: impl Into<GameId>, uid: impl Into<String>) -> Self {
        Self {
            game: game.into(),
            uid: uid.into(),
            region: None,
            nickname: None,
            tz_offset_min: DEFAULT_TZ_OFFSET_MIN,
        }
    }
}

/// 已入库的一条抽卡记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GachaRecord {
    /// 主键。
    pub id: RecordId,
    /// 所属账号。
    pub account_id: AccountId,
    /// 记录内容。
    #[serde(flatten)]
    pub data: NewRecord,
    /// 来自哪个导入批次。
    pub batch_id: Option<BatchId>,
}

/// 待写入的一条记录（已规范化）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRecord {
    /// 规范化后的卡池类型（即保底共享组）。
    pub pool_type: PoolTypeId,
    /// 具体哪一期卡池，可空。
    pub banner_id: Option<String>,
    /// 官方记录 ID；没有时为 `None`，按自然键去重。
    pub ext_id: Option<String>,
    /// 物品 ID。
    pub item_id: Option<String>,
    /// 物品名（记录原文）。
    pub item_name: String,
    /// 物品类型（角色 / 武器等，记录原文）。
    pub item_type: Option<String>,
    /// 星级，按游戏内显示（原神 3–5，方舟 1–6）。
    pub rarity: u8,
    /// 抽卡时间（UTC 毫秒）。
    pub pulled_at: i64,
    /// 同一秒、同一卡池类型内的顺序，从 0 开始（十连）。
    pub seq_in_sec: u32,
    /// 是否计入抽数与保底；免费抽、赠送条目为 `false`。
    pub is_pull: bool,
    /// 原始字段（JSON 文本），导出时无损还原。
    pub raw_json: String,
}

/// 导入来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportSource {
    /// 文件导入。
    File,
    /// 抽卡记录链接拉取。
    Url,
    /// 鹰角账号 token 拉取。
    Token,
}

impl ImportSource {
    /// 存进数据库用的字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Url => "url",
            Self::Token => "token",
        }
    }

    /// 从数据库字符串还原。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "file" => Some(Self::File),
            "url" => Some(Self::Url),
            "token" => Some(Self::Token),
            _ => None,
        }
    }
}

/// 一次导入的元信息。每个账号各写一条 `import_batch`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportMeta {
    /// 来源。
    pub source: ImportSource,
    /// 文件格式（文件导入时）。
    pub format: Option<FormatId>,
    /// 文件名（不含目录）。
    pub file_name: Option<String>,
    /// 开始时间（UTC 毫秒）。
    pub started_at: i64,
}

/// 一个账号要写入的记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountImport {
    /// 账号；不存在时自动创建（FR-ACC-02）。
    pub account: NewAccount,
    /// 记录。
    pub records: Vec<NewRecord>,
}

/// 单个账号的写入结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountInsert {
    /// 账号。
    pub account: Account,
    /// 本次是否新建了账号。
    pub created: bool,
    /// 导入批次。
    pub batch_id: BatchId,
    /// 新增条数。
    pub added: u32,
    /// 重复而跳过的条数。
    pub skipped: u32,
}

/// 一次写入（单事务）的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsertSummary {
    /// 各账号的结果，顺序与输入一致。
    pub accounts: Vec<AccountInsert>,
}

impl InsertSummary {
    /// 新增总数。
    pub fn added(&self) -> u32 {
        self.accounts.iter().map(|a| a.added).sum()
    }

    /// 跳过总数。
    pub fn skipped(&self) -> u32 {
        self.accounts.iter().map(|a| a.skipped).sum()
    }
}

/// 增量拉取的游标：某账号某卡池类型下最新的一条记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// 最新记录的官方 ID。
    pub ext_id: Option<String>,
    /// 最新记录的时间（UTC 毫秒）。
    pub pulled_at: i64,
    /// 最新记录的同秒序号。
    pub seq_in_sec: u32,
}

/// 记录排序方向（按时间）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// 最新的在前（记录列表默认）。
    #[default]
    NewestFirst,
    /// 最早的在前。
    OldestFirst,
}

/// 记录列表查询（FR-REC-01、FR-REC-03）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordQuery {
    /// 账号。
    pub account_id: AccountId,
    /// 卡池类型，空表示全部。
    #[serde(default)]
    pub pool_types: Vec<PoolTypeId>,
    /// 星级，空表示全部。
    #[serde(default)]
    pub rarities: Vec<u8>,
    /// 起始时间（含，UTC 毫秒）。
    pub from: Option<i64>,
    /// 结束时间（不含，UTC 毫秒）。
    pub to: Option<i64>,
    /// 按物品名模糊搜索。
    pub name: Option<String>,
    /// 排序。
    #[serde(default)]
    pub order: SortOrder,
    /// 页码，从 0 开始。
    #[serde(default)]
    pub page: u32,
    /// 每页条数（20 / 50 / 100）。
    pub page_size: u32,
}

impl RecordQuery {
    /// 某账号的全部记录，第一页。
    pub fn new(account_id: AccountId, page_size: u32) -> Self {
        Self {
            account_id,
            pool_types: Vec::new(),
            rarities: Vec::new(),
            from: None,
            to: None,
            name: None,
            order: SortOrder::default(),
            page: 0,
            page_size,
        }
    }
}

/// 一页结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    /// 本页内容。
    pub items: Vec<T>,
    /// 满足条件的总条数。
    pub total: u64,
    /// 页码。
    pub page: u32,
    /// 每页条数。
    pub page_size: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_source_round_trip() {
        for s in [ImportSource::File, ImportSource::Url, ImportSource::Token] {
            assert_eq!(ImportSource::parse(s.as_str()), Some(s));
        }
        assert_eq!(ImportSource::parse("ftp"), None);
    }

    #[test]
    fn insert_summary_totals() {
        let acc = |added, skipped| AccountInsert {
            account: Account {
                id: "a".into(),
                game: "genshin".into(),
                uid: "1".into(),
                region: None,
                nickname: None,
                tz_offset_min: 480,
                created_at: 0,
            },
            created: false,
            batch_id: "b".into(),
            added,
            skipped,
        };
        let s = InsertSummary {
            accounts: vec![acc(3, 1), acc(2, 5)],
        };
        assert_eq!((s.added(), s.skipped()), (5, 6));
    }

    #[test]
    fn defaults() {
        let a = NewAccount::new("genshin", "100000001");
        assert_eq!(a.tz_offset_min, DEFAULT_TZ_OFFSET_MIN);
        let q = RecordQuery::new("acc".into(), 20);
        assert_eq!(
            (q.page, q.page_size, q.order),
            (0, 20, SortOrder::NewestFirst)
        );
    }
}
