//! 转换器与拉取器的输入输出：解码得到的账号与记录（未规范化），以及导出用的数据。

use serde::{Deserialize, Serialize};

use crate::ids::{GameId, PoolTypeId};
use crate::model::{Account, GachaRecord};

/// 记录的卡池类型。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PoolRef {
    /// 官方卡池类型代码（如原神 `301`、`400`），由游戏插件映射成 [`PoolTypeId`]。
    Official(String),
    /// 已经是本软件的卡池类型（GNJ、自有 CSV）。
    Normalized(PoolTypeId),
}

/// 解码或拉取得到的一条记录。时间已换算成 UTC，其他字段尚未按游戏规范化。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedRecord {
    /// 卡池类型。
    pub pool: PoolRef,
    /// 具体卡池 ID（方舟 `poolId`、崩铁 `gacha_id` 等）。
    pub banner_id: Option<String>,
    /// 官方记录 ID。
    pub ext_id: Option<String>,
    /// 物品 ID。
    pub item_id: Option<String>,
    /// 物品名。
    pub item_name: String,
    /// 物品类型。
    pub item_type: Option<String>,
    /// 星级（游戏内显示的星数）。
    pub rarity: u8,
    /// 时间（UTC 毫秒）。
    pub pulled_at: i64,
    /// 来源自带的同秒序号（方舟 `pos`）；为空时由管线按顺序计算。
    pub seq: Option<u32>,
    /// 是否计入抽数与保底。
    pub is_pull: bool,
    /// 原始字段。
    pub raw: serde_json::Value,
    /// 在文件中的位置（行号或 JSON 路径），用于导入报告。
    pub location: String,
}

/// 无法解码、被跳过的记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordFailure {
    /// 位置（行号或 JSON 路径）。
    pub location: String,
    /// 原因。
    pub reason: String,
}

impl RecordFailure {
    /// 构造。
    pub fn new(location: impl Into<String>, reason: impl ToString) -> Self {
        Self {
            location: location.into(),
            reason: reason.to_string(),
        }
    }
}

/// 解码或拉取得到的一个账号。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportedAccount {
    /// 游戏。
    pub game: GameId,
    /// UID。
    pub uid: String,
    /// 服务器。
    pub region: Option<String>,
    /// 昵称。
    pub nickname: Option<String>,
    /// 服务器时区偏移（分钟）。
    pub tz_offset_min: i32,
    /// 记录语言（如 `zh-cn`）。
    pub lang: Option<String>,
    /// 记录。
    pub records: Vec<ImportedRecord>,
    /// 解码失败的记录。
    pub failures: Vec<RecordFailure>,
}

impl ImportedAccount {
    /// 空账号。
    pub fn new(game: impl Into<GameId>, uid: impl Into<String>, tz_offset_min: i32) -> Self {
        Self {
            game: game.into(),
            uid: uid.into(),
            region: None,
            nickname: None,
            tz_offset_min,
            lang: None,
            records: Vec::new(),
            failures: Vec::new(),
        }
    }
}

/// 导出用：一个账号及其全部记录（按时间顺序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportAccount {
    /// 账号。
    pub account: Account,
    /// 记录。
    pub records: Vec<GachaRecord>,
}

/// 读取 JSON 中「字符串或数字」字段为字符串，UIGF 等格式两种写法都有。
pub fn json_text(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_text_accepts_string_number_bool() {
        assert_eq!(json_text(&json!("1")).as_deref(), Some("1"));
        assert_eq!(json_text(&json!(5)).as_deref(), Some("5"));
        assert_eq!(json_text(&json!(true)).as_deref(), Some("true"));
        assert_eq!(json_text(&json!(null)), None);
        assert_eq!(json_text(&json!([1])), None);
    }

    #[test]
    fn pool_ref_serde() {
        let p = PoolRef::Official("301".into());
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            json!({"kind": "official", "value": "301"})
        );
        let n: PoolRef =
            serde_json::from_value(json!({"kind": "normalized", "value": "character"})).unwrap();
        assert_eq!(n, PoolRef::Normalized("character".into()));
    }

    #[test]
    fn constructors() {
        let a = ImportedAccount::new("genshin", "1", 480);
        assert!(a.records.is_empty() && a.failures.is_empty());
        let f = RecordFailure::new("line 2", "bad");
        assert_eq!((f.location.as_str(), f.reason.as_str()), ("line 2", "bad"));
    }
}
