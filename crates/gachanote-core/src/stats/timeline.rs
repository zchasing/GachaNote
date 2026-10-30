//! 时间线标签的派生数据（UI/UX 文档 3.6，F-19）：按日星级堆叠与卡池期数。
//!
//! 期数优先按元数据包的卡池起止时间划分；记录落不进任何一期时，有卡池 ID 的
//! （方舟、终末地）按卡池 ID 归组，其余按时间自动分段。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::analysis::{civil_from_days, DAY_MS};
use super::TopPull;
use crate::meta::Banner;
use crate::model::NewRecord;

/// 自动分段：相邻两条记录相隔超过这么多天就另起一段。
pub const AUTO_GAP_DAYS: i64 = 3;
/// 自动分段：一段最长这么多天（大致一期卡池的长度）。
pub const AUTO_MAX_DAYS: i64 = 21;

/// 按日的抽数，按星级分三档。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Daily {
    /// 第一天（服务器时区），如 `2026-10-01`；没有记录时为空。
    pub start: String,
    /// 从第一天起每天一项：`[最高星, 次高星, 其他]` 的数量（只算 `is_pull`）。
    pub counts: Vec<[u32; 3]>,
}

/// 按日汇总一个或多个卡池的抽数：从第一条记录所在日到 `until` 所在日（取两者较晚），
/// 中间没有抽卡的日子补 0。日期按 `tz_offset_min` 换算。
pub fn daily(records: &[&NewRecord], rarity_top: u8, tz_offset_min: i32, until: i64) -> Daily {
    let offset = i64::from(tz_offset_min) * 60_000;
    let day = |ms: i64| (ms + offset).div_euclid(DAY_MS);
    let pulls: Vec<&&NewRecord> = records.iter().filter(|r| r.is_pull).collect();
    let Some(first) = pulls.iter().map(|r| day(r.pulled_at)).min() else {
        return Daily::default();
    };
    let last = pulls
        .iter()
        .map(|r| day(r.pulled_at))
        .max()
        .unwrap_or(first)
        .max(day(until));
    let len = usize::try_from(last - first + 1).unwrap_or(0);
    let mut counts = vec![[0u32; 3]; len];
    for r in pulls {
        let i = usize::try_from(day(r.pulled_at) - first).unwrap_or(0);
        let tier = if r.rarity >= rarity_top {
            0
        } else if r.rarity + 1 == rarity_top {
            1
        } else {
            2
        };
        counts[i][tier] += 1;
    }
    let (y, m, d) = civil_from_days(first);
    Daily {
        start: format!("{y:04}-{m:02}-{d:02}"),
        counts,
    }
}

/// 一期是怎么划分出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeriodSource {
    /// 元数据包里的一期卡池。
    Banner,
    /// 记录自带的卡池 ID（元数据包里没有这一期）。
    Pool,
    /// 没有卡池信息，按时间自动分段。
    Auto,
}

/// 时间线上的一期。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Period {
    /// 划分方式。
    pub source: PeriodSource,
    /// 卡池 ID（元数据包或记录里的）；自动分段为空。
    pub id: Option<String>,
    /// 开始时间：卡池开放时间，或本段第一条记录的时间（UTC 毫秒）。
    pub start: i64,
    /// 结束时间：卡池关闭时间，或本段最后一条记录的时间。
    pub end: i64,
    /// 本期抽数（只算 `is_pull`）。
    pub pulls: u32,
    /// 本期出的最高星，按时间顺序。
    pub tops: Vec<TopPull>,
    /// 本期第一条记录在输入中的下标（取卡池名等原文用）；未参与的卡池为空。
    pub first_record: Option<usize>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Banner(usize),
    Pool(String),
    Auto(usize),
}

/// 把一个卡池类型的记录分成期，最新的一期在前。
///
/// - `records`：该卡池类型的全部记录（顺序与计算 `tops` 时相同）；
/// - `tops`：[`super::pool_stats`] 得出的最高星序列，`index` 指向 `records`；
/// - `banners`：元数据包里该卡池类型的各期，`now` 之前开放、且在第一条记录之后结束的
///   都会列出（没抽的期 `pulls` 为 0，界面显示为「未参与」）。
pub fn periods(
    records: &[&NewRecord],
    tops: &[TopPull],
    banners: &[&Banner],
    now: i64,
) -> Vec<Period> {
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&i| (records[i].pulled_at, records[i].seq_in_sec));

    let banner_of = |r: &NewRecord| -> Option<usize> {
        r.banner_id
            .as_ref()
            .and_then(|id| banners.iter().position(|b| &b.id == id))
            .or_else(|| {
                banners
                    .iter()
                    .position(|b| b.start <= r.pulled_at && r.pulled_at < b.end)
            })
    };

    // 每条记录归到哪一期
    let mut keys: Vec<Option<Key>> = vec![None; records.len()];
    let mut auto = 0usize;
    let mut auto_span: Option<(i64, i64)> = None;
    for &i in &order {
        let r = records[i];
        let key = if let Some(b) = banner_of(r) {
            Key::Banner(b)
        } else if let Some(id) = &r.banner_id {
            Key::Pool(id.clone())
        } else {
            let t = r.pulled_at;
            match auto_span {
                Some((start, last))
                    if t - last <= AUTO_GAP_DAYS * DAY_MS
                        && t - start <= AUTO_MAX_DAYS * DAY_MS =>
                {
                    auto_span = Some((start, t));
                }
                Some(_) => {
                    auto += 1;
                    auto_span = Some((t, t));
                }
                None => auto_span = Some((t, t)),
            }
            Key::Auto(auto)
        };
        keys[i] = Some(key);
    }

    let mut map: BTreeMap<Key, Period> = BTreeMap::new();
    for &i in &order {
        let Some(key) = keys[i].clone() else { continue };
        let r = records[i];
        let p = map.entry(key.clone()).or_insert_with(|| {
            let (source, id, start, end) = match &key {
                Key::Banner(b) => (
                    PeriodSource::Banner,
                    Some(banners[*b].id.clone()),
                    banners[*b].start,
                    banners[*b].end,
                ),
                Key::Pool(id) => (
                    PeriodSource::Pool,
                    Some(id.clone()),
                    r.pulled_at,
                    r.pulled_at,
                ),
                Key::Auto(_) => (PeriodSource::Auto, None, r.pulled_at, r.pulled_at),
            };
            Period {
                source,
                id,
                start,
                end,
                pulls: 0,
                tops: Vec::new(),
                first_record: Some(i),
            }
        });
        if p.source != PeriodSource::Banner {
            p.end = p.end.max(r.pulled_at);
        }
        if r.is_pull {
            p.pulls += 1;
        }
    }
    for t in tops {
        if let Some(Some(key)) = keys.get(t.index) {
            if let Some(p) = map.get_mut(key) {
                p.tops.push(t.clone());
            }
        }
    }

    // 没抽过的期：第一条记录之后结束、现在之前开放的
    if let Some(first) = order.first().map(|&i| records[i].pulled_at) {
        for (b, banner) in banners.iter().enumerate() {
            if banner.end > first && banner.start <= now {
                map.entry(Key::Banner(b)).or_insert_with(|| Period {
                    source: PeriodSource::Banner,
                    id: Some(banner.id.clone()),
                    start: banner.start,
                    end: banner.end,
                    pulls: 0,
                    tops: Vec::new(),
                    first_record: None,
                });
            }
        }
    }

    let mut out: Vec<Period> = map.into_values().collect();
    out.sort_by(|a, b| b.start.cmp(&a.start).then(b.end.cmp(&a.end)));
    out
}

#[cfg(test)]
mod tests;
