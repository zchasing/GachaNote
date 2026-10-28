//! 统计引擎（架构文档第 6 节）：纯函数，输入按时间排序的记录、规则模板和元数据，
//! 输出保底状态与核心指标，不访问数据库和网络。
//!
//! 对每个保底组（即规范化后的卡池类型）顺序扫描一次记录，维护 [`PityStatus`]，
//! 每遇到一只最高星生成一条 [`TopPull`]，所有核心指标都由这个序列派生。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::PoolTypeId;
use crate::meta::{Classified, GameMeta, UpStatus};
use crate::model::{GachaRecord, NewRecord};
use crate::plugin::GamePlugin;
use crate::rules::{rule_at, PityScope, PoolRule, UpRule};

pub mod analysis;
pub mod luck;
pub mod predict;

/// 一只最高星。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopPull {
    /// 在输入记录中的下标。
    pub index: usize,
    /// 物品名。
    pub item_name: String,
    /// 时间（UTC 毫秒）。
    pub pulled_at: i64,
    /// 所属卡池。
    pub banner_id: Option<String>,
    /// 花费抽数（距上一只最高星）。
    pub pulls: u32,
    /// 是否 UP。
    pub up: UpStatus,
    /// UP 判定是否为估算。
    pub estimated: bool,
    /// 出这只时是否处于大保底（或命定值已满），这类不计入歪率分母。
    pub was_guaranteed: bool,
}

/// 当前保底状态（FR-STAT-01）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PityStatus {
    /// 距上次最高星已垫抽数。
    pub since_top: u32,
    /// 距硬保底剩余抽数。
    pub remaining_to_hard: u32,
    /// 下一只最高星是否大保底。
    pub guaranteed: bool,
    /// 大保底状态是否为推断（之前有 UP 未知或估算的最高星）。
    pub guarantee_uncertain: bool,
    /// 命定值（有定轨机制时），由于记录里看不出定轨对象，为估算值。
    pub path_points: Option<u8>,
    /// 距上次 UP 的抽数（有「N 抽必出 UP」机制时）。
    pub since_up: Option<u32>,
    /// 当期卡池已抽数（井 / 自选与每期重置用）。
    pub banner_pulls: u32,
}

/// 歪率：分子与分母（FR-STAT-03）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossRate {
    /// 歪的次数。
    pub lost: u32,
    /// 小保底状态下出最高星的次数（UP 已知的）。
    pub total: u32,
}

impl LossRate {
    /// 歪率；分母为 0 时为 `None`。
    pub fn rate(&self) -> Option<f64> {
        (self.total > 0).then(|| f64::from(self.lost) / f64::from(self.total))
    }
}

/// 一个保底组的统计。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolStats {
    /// 卡池类型。
    pub pool_type: PoolTypeId,
    /// 总抽数（只算 `is_pull`）。
    pub total_pulls: u32,
    /// 不计抽数的条目数（免费抽、赠送）。
    pub free_count: u32,
    /// 各星级数量（只算 `is_pull`，与总抽数同口径）。
    pub rarity_counts: BTreeMap<u8, u32>,
    /// 最高星序列，按时间顺序。
    pub tops: Vec<TopPull>,
    /// 平均出最高星抽数。
    pub avg_pulls_per_top: Option<f64>,
    /// UP 数。
    pub up_count: u32,
    /// 平均出 UP 抽数（FR-STAT-02）；常驻池为空。
    pub avg_pulls_per_up: Option<f64>,
    /// 歪率；常驻池为空。
    pub loss: Option<LossRate>,
    /// UP 未知的最高星数（FR-META-04）。
    pub unknown_count: u32,
    /// UP 判定为估算的最高星数。
    pub estimated_count: u32,
    /// 当前保底状态。
    pub pity: PityStatus,
}

/// 一个账号的统计。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountStats {
    /// 各保底组，顺序同插件的规则模板。
    pub pools: Vec<PoolStats>,
    /// 总抽数。
    pub total_pulls: u32,
    /// 各星级数量。
    pub rarity_counts: BTreeMap<u8, u32>,
    /// 没有规则模板、未参与统计的记录数。
    pub unrated: u32,
}

/// 计算一个保底组的统计。`records` 只含该卡池类型的记录；未排序时内部按时间排序。
/// 没有该卡池类型的规则时返回 `None`。
pub fn pool_stats(
    pool_type: &PoolTypeId,
    rules: &[PoolRule],
    records: &[&NewRecord],
    classify: &dyn Fn(&NewRecord) -> Classified,
) -> Option<PoolStats> {
    let latest = rule_at(rules, pool_type, None)?;
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by_key(|&i| (records[i].pulled_at, records[i].seq_in_sec));

    let mut s = PoolStats {
        pool_type: pool_type.clone(),
        total_pulls: 0,
        free_count: 0,
        rarity_counts: BTreeMap::new(),
        tops: Vec::new(),
        avg_pulls_per_top: None,
        up_count: 0,
        avg_pulls_per_up: None,
        loss: latest.up.has_up().then(LossRate::default),
        unknown_count: 0,
        estimated_count: 0,
        pity: PityStatus::default(),
    };
    let mut path_points = 0u8;
    let mut since_up = 0u32;
    let mut current_banner: Option<String> = None;
    let mut up_segment = 0u32;
    let mut up_pulls_total = 0u64;

    for i in order {
        let rec = records[i];
        let rule = rule_at(rules, pool_type, Some(rec.pulled_at)).unwrap_or(latest);
        if !rec.is_pull {
            s.free_count += 1;
            continue;
        }
        *s.rarity_counts.entry(rec.rarity).or_default() += 1;

        let is_top = rec.rarity >= rule.rarity_top;
        let tracks_banner = rule.pity_scope == PityScope::PerBanner
            || rule.path_max().is_some()
            || rule.up_within().is_some_and(|(_, per)| per)
            || rule.spark().is_some();
        let classified = (is_top || tracks_banner).then(|| classify(rec));
        if tracks_banner {
            let banner = classified.as_ref().and_then(|c| c.banner_id.clone());
            if banner.is_some() && banner != current_banner {
                if current_banner.is_some() {
                    s.pity.banner_pulls = 0;
                    path_points = 0;
                    if rule.pity_scope == PityScope::PerBanner {
                        s.pity.since_top = 0;
                        s.pity.guaranteed = false;
                    }
                    if rule.up_within().is_some_and(|(_, per)| per) {
                        since_up = 0;
                    }
                }
                current_banner = banner;
            }
        }

        s.total_pulls += 1;
        s.pity.since_top += 1;
        s.pity.banner_pulls += 1;
        since_up += 1;
        up_segment += 1;

        if !is_top {
            continue;
        }
        let c = classified.unwrap_or_else(|| classify(rec));
        let path_full = rule.path_max().is_some_and(|max| path_points >= max);
        let was_guaranteed = s.pity.guaranteed || path_full;
        s.tops.push(TopPull {
            index: i,
            item_name: rec.item_name.clone(),
            pulled_at: rec.pulled_at,
            banner_id: c.banner_id.clone(),
            pulls: s.pity.since_top,
            up: c.up,
            estimated: c.estimated,
            was_guaranteed,
        });
        s.pity.since_top = 0;
        if c.estimated {
            s.estimated_count += 1;
        }

        if !rule.up.has_up() {
            continue;
        }
        match c.up {
            UpStatus::Up => {
                s.up_count += 1;
                up_pulls_total += u64::from(up_segment);
                up_segment = 0;
                since_up = 0;
            }
            UpStatus::NotUp => {}
            UpStatus::Unknown => s.unknown_count += 1,
        }
        if c.up != UpStatus::Unknown && !was_guaranteed {
            if let Some(loss) = &mut s.loss {
                loss.total += 1;
                if c.up == UpStatus::NotUp {
                    loss.lost += 1;
                }
            }
        }
        if let UpRule::FiftyFifty {
            guarantee_next: true,
            ..
        } = rule.up
        {
            s.pity.guaranteed = !s.pity.guaranteed && c.up == UpStatus::NotUp;
            // 这只的 UP 未知或为估算时，下一只是否大保底只能推断
            s.pity.guarantee_uncertain = c.up == UpStatus::Unknown || c.estimated;
        }
        if let Some(max) = rule.path_max() {
            path_points = match c.up {
                _ if path_full => 0,
                UpStatus::Up => 0,
                UpStatus::NotUp => (path_points + 1).min(max),
                UpStatus::Unknown => path_points,
            };
        }
    }

    s.pity.remaining_to_hard = latest.curve.hard.saturating_sub(s.pity.since_top);
    s.pity.path_points = latest.path_max().map(|_| path_points);
    s.pity.since_up = latest.up_within().map(|_| since_up);
    if !s.tops.is_empty() {
        let sum: u64 = s.tops.iter().map(|t| u64::from(t.pulls)).sum();
        s.avg_pulls_per_top = Some(sum as f64 / s.tops.len() as f64);
    }
    if latest.up.has_up() && s.up_count > 0 {
        s.avg_pulls_per_up = Some(up_pulls_total as f64 / f64::from(s.up_count));
    }
    Some(s)
}

/// 计算一个账号的统计：按卡池类型分组，逐组扫描。
pub fn account_stats(
    plugin: &dyn GamePlugin,
    meta: &GameMeta,
    records: &[GachaRecord],
) -> AccountStats {
    let mut groups: BTreeMap<&PoolTypeId, Vec<&NewRecord>> = BTreeMap::new();
    for r in records {
        groups.entry(&r.data.pool_type).or_default().push(&r.data);
    }
    let classify = |r: &NewRecord| plugin.classify(r, meta);
    let mut pools = Vec::new();
    let mut seen: Vec<&PoolTypeId> = Vec::new();
    for rule in plugin.rules() {
        if seen.contains(&&rule.pool_type) {
            continue;
        }
        seen.push(&rule.pool_type);
        let recs = groups
            .get(&rule.pool_type)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Some(p) = pool_stats(&rule.pool_type, plugin.rules(), recs, &classify) {
            pools.push(p);
        }
    }
    let mut rarity_counts = BTreeMap::new();
    for p in &pools {
        for (k, v) in &p.rarity_counts {
            *rarity_counts.entry(*k).or_default() += v;
        }
    }
    let rated: usize = groups
        .iter()
        .filter(|(k, _)| seen.contains(k))
        .map(|(_, v)| v.len())
        .sum();
    AccountStats {
        total_pulls: pools.iter().map(|p| p.total_pulls).sum(),
        rarity_counts,
        unrated: u32::try_from(records.len() - rated).unwrap_or(u32::MAX),
        pools,
    }
}

/// 记录列表中一条记录的标注（FR-REC-02）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordMark {
    /// 当次抽数：最高星为距上一只最高星的抽数，次高星为距上一只次高星及以上的抽数；
    /// 其他星级、免费抽和没有规则的记录为空。
    pub pulls: Option<u32>,
    /// 最高星是否 UP；常驻池和其他星级为空。
    pub up: Option<UpStatus>,
    /// UP 判定是否为估算。
    pub estimated: bool,
}

/// 计算一个账号全部记录的标注，结果与 `records` 一一对应。
///
/// 最高星的抽数与 UP 直接取自 [`pool_stats`] 的最高星序列，与统计页口径一致
/// （包括每期重置的卡池）。
pub fn record_marks(
    plugin: &dyn GamePlugin,
    meta: &GameMeta,
    records: &[GachaRecord],
) -> Vec<RecordMark> {
    let mut marks = vec![RecordMark::default(); records.len()];
    let mut groups: BTreeMap<&PoolTypeId, Vec<usize>> = BTreeMap::new();
    for (i, r) in records.iter().enumerate() {
        groups.entry(&r.data.pool_type).or_default().push(i);
    }
    let classify = |r: &NewRecord| plugin.classify(r, meta);
    for (pool_type, idx) in groups {
        let refs: Vec<&NewRecord> = idx.iter().map(|&i| &records[i].data).collect();
        let Some(stats) = pool_stats(pool_type, plugin.rules(), &refs, &classify) else {
            continue;
        };
        let has_up = rule_at(plugin.rules(), pool_type, None).is_some_and(|r| r.up.has_up());
        for t in &stats.tops {
            marks[idx[t.index]] = RecordMark {
                pulls: Some(t.pulls),
                up: has_up.then_some(t.up),
                estimated: has_up && t.estimated,
            };
        }
        let mut order: Vec<usize> = (0..refs.len()).collect();
        order.sort_by_key(|&k| (refs[k].pulled_at, refs[k].seq_in_sec));
        let mut since_second = 0u32;
        for k in order {
            let rec = refs[k];
            if !rec.is_pull {
                continue;
            }
            let top = rule_at(plugin.rules(), pool_type, Some(rec.pulled_at))
                .map_or(u8::MAX, |r| r.rarity_top);
            since_second += 1;
            if rec.rarity >= top {
                since_second = 0;
            } else if rec.rarity + 1 == top {
                marks[idx[k]].pulls = Some(since_second);
                since_second = 0;
            }
        }
    }
    marks
}

#[cfg(test)]
mod tests;
