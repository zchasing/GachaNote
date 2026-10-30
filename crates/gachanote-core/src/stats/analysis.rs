//! 分析页的派生数据：成就徽章与按月趋势（UI/UX 文档 3.5，F-15）。
//!
//! 都只由 [`PoolStats`] 的最高星序列与记录时间派生，不另外扫描保底状态。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::PoolStats;
use crate::meta::UpStatus;
use crate::model::NewRecord;
use crate::rules::PoolRule;

const DAY_MS: i64 = 86_400_000;

/// 成就种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AchievementKind {
    /// 欧皇时刻：某只最高星的抽数不超过硬保底的一定比例。
    Lucky,
    /// 非酋时刻：进入软保底若干抽后才出。
    Unlucky,
    /// 不歪连胜：连续若干只最高星都是 UP。
    Streak,
    /// 漫长等待：两次出最高星相隔若干天。
    LongWait,
}

/// 一个成就徽章。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    /// 种类。
    pub kind: AchievementKind,
    /// 是否达成。
    pub done: bool,
    /// 当前最好成绩：最少抽数、最多抽数、最长连胜或最长间隔天数。
    pub value: u32,
    /// 达成条件（与 `value` 同单位）。
    pub threshold: u32,
    /// 对应的物品名（欧皇 / 非酋时刻）。
    pub item_name: Option<String>,
}

/// 按规则模板的阈值计算成就；没有最高星时为空。常驻池没有「不歪连胜」。
pub fn achievements(rule: &PoolRule, stats: &PoolStats) -> Vec<Achievement> {
    let tops = &stats.tops;
    let (Some(min), Some(max)) = (
        tops.iter().min_by_key(|t| t.pulls),
        tops.iter().max_by_key(|t| t.pulls),
    ) else {
        return Vec::new();
    };
    let a = rule.achievements;
    let hard = rule.curve.hard;
    let lucky_line = ((f64::from(hard) * a.lucky_ratio).floor() as u32).max(1);
    let unlucky_line = match rule.curve.soft_start {
        Some(s) => (s + a.unlucky_soft_pulls.saturating_sub(1)).min(hard),
        None => hard,
    };
    let mut out = vec![
        Achievement {
            kind: AchievementKind::Lucky,
            done: min.pulls <= lucky_line,
            value: min.pulls,
            threshold: lucky_line,
            item_name: Some(min.item_name.clone()),
        },
        Achievement {
            kind: AchievementKind::Unlucky,
            done: max.pulls >= unlucky_line,
            value: max.pulls,
            threshold: unlucky_line,
            item_name: Some(max.item_name.clone()),
        },
    ];
    if rule.up.has_up() {
        let mut best = 0u32;
        let mut run = 0u32;
        for t in tops {
            match t.up {
                UpStatus::Up => {
                    run += 1;
                    best = best.max(run);
                }
                UpStatus::NotUp => run = 0,
                // UP 未知的不打断也不计入
                UpStatus::Unknown => {}
            }
        }
        out.push(Achievement {
            kind: AchievementKind::Streak,
            done: best >= a.streak,
            value: best,
            threshold: a.streak,
            item_name: None,
        });
    }
    let mut times: Vec<i64> = tops.iter().map(|t| t.pulled_at).collect();
    times.sort_unstable();
    let gap = times
        .windows(2)
        .map(|w| (w[1] - w[0]) / DAY_MS)
        .max()
        .unwrap_or(0);
    let gap = u32::try_from(gap).unwrap_or(u32::MAX);
    out.push(Achievement {
        kind: AchievementKind::LongWait,
        done: times.len() >= 2 && gap >= a.long_wait_days,
        value: gap,
        threshold: a.long_wait_days,
        item_name: None,
    });
    out
}

/// 一个月的抽卡情况。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthBucket {
    /// 年月（服务器时区），如 `2026-10`。
    pub month: String,
    /// 抽数（只算 `is_pull`）。
    pub pulls: u32,
    /// 最高星数。
    pub tops: u32,
    /// 其中 UP 数。
    pub ups: u32,
}

/// 按月汇总抽数与出金数（FR-ANA-02）。`records` 为一个或多个卡池的记录，
/// `stats` 为这些卡池的统计（提供最高星序列）；月份按 `tz_offset_min` 换算，
/// 从第一个有记录的月份到最后一个，中间没有抽卡的月份补 0。
pub fn monthly(
    records: &[&NewRecord],
    stats: &[&PoolStats],
    tz_offset_min: i32,
) -> Vec<MonthBucket> {
    let offset = i64::from(tz_offset_min) * 60_000;
    let key = |ms: i64| month_of(ms + offset);
    let mut map: BTreeMap<(i32, u32), MonthBucket> = BTreeMap::new();
    for r in records.iter().filter(|r| r.is_pull) {
        bucket(&mut map, key(r.pulled_at)).pulls += 1;
    }
    for t in stats.iter().flat_map(|s| s.tops.iter()) {
        let b = bucket(&mut map, key(t.pulled_at));
        b.tops += 1;
        if t.up == UpStatus::Up {
            b.ups += 1;
        }
    }
    let (Some(&first), Some(&last)) = (map.keys().next(), map.keys().next_back()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut cur = first;
    while cur <= last {
        bucket(&mut map, cur);
        out.extend(map.remove(&cur));
        cur = if cur.1 == 12 {
            (cur.0 + 1, 1)
        } else {
            (cur.0, cur.1 + 1)
        };
    }
    out
}

fn bucket(map: &mut BTreeMap<(i32, u32), MonthBucket>, k: (i32, u32)) -> &mut MonthBucket {
    map.entry(k).or_insert_with(|| MonthBucket {
        month: format!("{:04}-{:02}", k.0, k.1),
        pulls: 0,
        tops: 0,
        ups: 0,
    })
}

/// UTC 毫秒（已加时区偏移）所在的年、月。
fn month_of(ms: i64) -> (i32, u32) {
    // Howard Hinnant 的 civil_from_days 算法，避免为此引入日期库
    let days = ms.div_euclid(DAY_MS);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        i32::try_from(year).unwrap_or(i32::MAX),
        u32::try_from(month).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests;
