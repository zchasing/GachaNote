//! 卡池元数据的内存模型与 UP 判定。
//!
//! 元数据包的下载、校验与缓存在 `gachanote-meta`（M2）；这里只定义统计引擎需要的数据结构，
//! 让 `core` 不依赖具体来源。

use serde::{Deserialize, Serialize};

use crate::ids::{GameId, PoolTypeId};
use crate::model::NewRecord;

/// 一期卡池。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Banner {
    /// 卡池 ID（元数据包内唯一；方舟等记录自带卡池 ID 时与之相同）。
    pub id: String,
    /// 卡池类型。
    pub pool_type: PoolTypeId,
    /// 卡池名。
    pub name: String,
    /// 开始时间（含，UTC 毫秒）。
    pub start: i64,
    /// 结束时间（不含，UTC 毫秒）。
    pub end: i64,
    /// UP 最高星物品（物品 ID 或名称）。
    #[serde(default)]
    pub up_top: Vec<String>,
    /// UP 次高星物品。
    #[serde(default)]
    pub up_second: Vec<String>,
}

impl Banner {
    fn contains(&self, at: i64) -> bool {
        self.start <= at && at < self.end
    }
}

/// 一款游戏的元数据。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameMeta {
    /// 游戏。
    pub game: Option<GameId>,
    /// 历史卡池。
    #[serde(default)]
    pub banners: Vec<Banner>,
    /// 常驻最高星名单（物品 ID 或名称）。
    #[serde(default)]
    pub standard: Vec<String>,
}

/// 一只最高星是否 UP。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpStatus {
    /// 是 UP。
    Up,
    /// 歪了（不是 UP）。
    NotUp,
    /// 缺元数据，无法判断（FR-META-04）：不计入歪率。
    Unknown,
}

/// UP 判定结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classified {
    /// 落在哪一期卡池。
    pub banner_id: Option<String>,
    /// 是否 UP。
    pub up: UpStatus,
    /// 是否为估算（没找到卡池，按「常驻名单之外即 UP」判断）。
    pub estimated: bool,
}

fn matches(list: &[String], rec: &NewRecord) -> bool {
    list.iter()
        .any(|x| x == &rec.item_name || rec.item_id.as_deref().is_some_and(|id| id == x))
}

impl GameMeta {
    /// 记录所在的卡池：优先用记录自带的 `banner_id`，否则按时间与卡池类型查找。
    pub fn banner_of(&self, rec: &NewRecord) -> Option<&Banner> {
        if let Some(id) = &rec.banner_id {
            if let Some(b) = self.banners.iter().find(|b| &b.id == id) {
                return Some(b);
            }
        }
        self.banners
            .iter()
            .find(|b| b.pool_type == rec.pool_type && b.contains(rec.pulled_at))
    }

    /// 判断一只最高星是否 UP（架构文档 6.1）：
    ///
    /// 1. 找到卡池且有 UP 名单：在名单内即 UP；
    /// 2. 找不到卡池但有常驻名单：常驻之外即 UP，标为估算；
    /// 3. 都没有：UP 未知。
    pub fn classify(&self, rec: &NewRecord) -> Classified {
        let banner = self.banner_of(rec);
        let banner_id = banner
            .map(|b| b.id.clone())
            .or_else(|| rec.banner_id.clone());
        match banner {
            Some(b) if !b.up_top.is_empty() => Classified {
                banner_id,
                up: if matches(&b.up_top, rec) {
                    UpStatus::Up
                } else {
                    UpStatus::NotUp
                },
                estimated: false,
            },
            _ if !self.standard.is_empty() => Classified {
                banner_id,
                up: if matches(&self.standard, rec) {
                    UpStatus::NotUp
                } else {
                    UpStatus::Up
                },
                estimated: true,
            },
            _ => Classified {
                banner_id,
                up: UpStatus::Unknown,
                estimated: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(name: &str, at: i64) -> NewRecord {
        NewRecord {
            pool_type: "character".into(),
            banner_id: None,
            ext_id: None,
            item_id: Some(format!("id-{name}")),
            item_name: name.into(),
            item_type: None,
            rarity: 5,
            pulled_at: at,
            seq_in_sec: 0,
            is_pull: true,
            raw_json: "{}".into(),
        }
    }

    fn meta() -> GameMeta {
        GameMeta {
            game: Some("genshin".into()),
            banners: vec![
                Banner {
                    id: "b1".into(),
                    pool_type: "character".into(),
                    name: "一期".into(),
                    start: 100,
                    end: 200,
                    up_top: vec!["温迪".into()],
                    up_second: vec![],
                },
                Banner {
                    id: "b2".into(),
                    pool_type: "character".into(),
                    name: "二期".into(),
                    start: 200,
                    end: 300,
                    up_top: vec!["id-可莉".into()],
                    up_second: vec![],
                },
                Banner {
                    id: "empty".into(),
                    pool_type: "character".into(),
                    name: "缺名单".into(),
                    start: 300,
                    end: 400,
                    up_top: vec![],
                    up_second: vec![],
                },
            ],
            standard: vec!["刻晴".into(), "迪卢克".into()],
        }
    }

    #[test]
    fn up_by_banner_list() {
        let m = meta();
        let c = m.classify(&rec("温迪", 150));
        assert_eq!(
            (c.banner_id.as_deref(), c.up, c.estimated),
            (Some("b1"), UpStatus::Up, false)
        );
        let c = m.classify(&rec("刻晴", 150));
        assert_eq!(c.up, UpStatus::NotUp);
        // 按物品 ID 匹配，结束时间不含
        let c = m.classify(&rec("可莉", 200));
        assert_eq!((c.banner_id.as_deref(), c.up), (Some("b2"), UpStatus::Up));
    }

    #[test]
    fn falls_back_to_standard_list() {
        let m = meta();
        let c = m.classify(&rec("温迪", 999));
        assert_eq!((c.up, c.estimated, c.banner_id), (UpStatus::Up, true, None));
        let c = m.classify(&rec("迪卢克", 999));
        assert_eq!((c.up, c.estimated), (UpStatus::NotUp, true));
        // 找到卡池但没有 UP 名单，也退化为估算
        let c = m.classify(&rec("温迪", 350));
        assert_eq!(
            (c.banner_id.as_deref(), c.up, c.estimated),
            (Some("empty"), UpStatus::Up, true)
        );
    }

    #[test]
    fn unknown_without_meta() {
        let mut r = rec("温迪", 150);
        r.banner_id = Some("ak_pool".into());
        let c = GameMeta::default().classify(&r);
        assert_eq!(
            (c.up, c.banner_id.as_deref()),
            (UpStatus::Unknown, Some("ak_pool"))
        );
    }

    #[test]
    fn record_banner_id_takes_priority() {
        let m = meta();
        let mut r = rec("可莉", 150);
        r.banner_id = Some("b2".into());
        assert_eq!(m.banner_of(&r).unwrap().id, "b2");
        r.banner_id = Some("missing".into());
        assert_eq!(m.banner_of(&r).unwrap().id, "b1");
    }
}
