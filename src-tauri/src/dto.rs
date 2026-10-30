//! 前后端之间传递的数据结构。
//!
//! 下层 crate 不依赖 specta，这里把领域类型换算成前端需要的形状（字段用 camelCase，
//! 映射表改为数组），由 tauri-specta 导出到 `ui/src/bindings.ts`。时间一律为 UTC 毫秒。

use std::collections::BTreeMap;

use gachanote_core::import::{FilePreview, ImportReport};
use gachanote_core::meta::UpStatus;
use gachanote_core::model::{Account, GachaRecord};
use gachanote_core::plugin::{FetchKind, GamePlugin, Vendor};
use gachanote_core::rules::{rule_at, Mechanic, PityScope, UpRule};
use gachanote_core::stats::analysis::{achievements, Achievement, AchievementKind, MonthBucket};
use gachanote_core::stats::luck::{luck, Luck, LuckMetric};
use gachanote_core::stats::predict::{predict, Prediction};
use gachanote_core::stats::timeline::PeriodSource;
use gachanote_core::stats::{AccountStats, LossRate, PoolStats, RecordMark, TopPull};
use gachanote_core::task::{ProgressEvent, Stage};
use gachanote_core::AppError;
use serde::{Deserialize, Serialize};
use specta::Type;

/// 命令失败时的错误：前端按 `code` 查 i18n 文案，`detail` 只在「详情」里显示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDto {
    /// 错误码，见 `AppError::code`。
    pub code: String,
    /// 英文说明（日志用）。
    pub message: String,
    /// 技术详情。
    pub detail: Option<String>,
}

impl From<AppError> for ErrorDto {
    fn from(e: AppError) -> Self {
        Self {
            code: e.code().into(),
            message: e.to_string(),
            detail: e.detail(),
        }
    }
}

/// 命令的返回类型。
pub type CmdResult<T> = Result<T, ErrorDto>;

/// UP 规则的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UpKind {
    /// 常驻池，没有 UP。
    None,
    /// 大小保底。
    FiftyFifty,
    /// 固定概率为 UP，没有大小保底（明日方舟、终末地）。
    Fixed,
}

/// 一种卡池类型的规则摘要（取最新一份规则），界面据此决定显示哪些指标卡。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PoolInfo {
    /// 卡池类型。
    pub id: String,
    /// 最高星级。
    pub rarity_top: u8,
    /// 硬保底。
    pub hard: u32,
    /// UP 规则。
    pub up: UpKind,
    /// 小保底时为 UP 的概率。
    pub up_rate: Option<f64>,
    /// 歪了之后是否进入大保底（是否显示大 / 小保底标签）。
    pub guarantee: bool,
    /// 井 / 自选所需抽数。
    pub spark: Option<u32>,
    /// N 抽内必出 UP。
    pub up_within: Option<u32>,
    /// 命定值上限。
    pub path_max: Option<u8>,
    /// 保底是否每期重置。
    pub per_banner: bool,
    /// 出一只最高星的理论平均抽数。
    pub expected_pulls: f64,
}

/// 游戏信息。名称由前端 i18n 按 `id` 查找。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GameDto {
    /// 游戏。
    pub id: String,
    /// 厂商：`hoyoverse` / `hypergryph` / `kuro` / `other`。
    pub vendor: String,
    /// 印象色。
    pub theme_color: String,
    /// 卡池类型，按界面显示顺序。
    pub pools: Vec<PoolInfo>,
    /// 在线拉取方式：`url` / `account_token`，没有时为空。
    pub fetch: Option<String>,
    /// 能否在线拉取（有拉取器）。
    pub can_fetch: bool,
    /// 支持导入的文件格式。
    pub formats: Vec<String>,
    /// 最高星级。
    pub rarity_top: u8,
}

impl GameDto {
    /// 从插件生成。
    pub fn from_plugin(p: &dyn GamePlugin) -> Self {
        let info = p.info();
        let pools: Vec<PoolInfo> = info
            .pool_types
            .iter()
            .filter_map(|t| rule_at(p.rules(), t, None))
            .map(|r| PoolInfo {
                id: r.pool_type.to_string(),
                rarity_top: r.rarity_top,
                hard: r.curve.hard,
                up: match r.up {
                    UpRule::None => UpKind::None,
                    UpRule::FiftyFifty { .. } => UpKind::FiftyFifty,
                    UpRule::Fixed { .. } => UpKind::Fixed,
                },
                up_rate: match r.up {
                    UpRule::None => None,
                    UpRule::FiftyFifty { rate, .. } | UpRule::Fixed { rate } => Some(rate),
                },
                guarantee: matches!(
                    r.up,
                    UpRule::FiftyFifty {
                        guarantee_next: true,
                        ..
                    }
                ),
                spark: r.spark(),
                up_within: r.up_within().map(|(n, _)| n),
                path_max: r.extras.iter().find_map(|m| match m {
                    Mechanic::EpitomizedPath { max } => Some(*max),
                    _ => None,
                }),
                per_banner: r.pity_scope == PityScope::PerBanner,
                expected_pulls: r.curve.expected_pulls(),
            })
            .collect();
        Self {
            id: info.id.to_string(),
            vendor: match info.vendor {
                Vendor::Hoyoverse => "hoyoverse",
                Vendor::Hypergryph => "hypergryph",
                Vendor::Kuro => "kuro",
                Vendor::Other => "other",
            }
            .into(),
            theme_color: info.theme_color,
            rarity_top: pools.iter().map(|p| p.rarity_top).max().unwrap_or(5),
            pools,
            fetch: info.fetch.map(|f| {
                match f {
                    FetchKind::Url => "url",
                    FetchKind::AccountToken => "account_token",
                }
                .into()
            }),
            can_fetch: info.fetch.is_some() && p.fetcher().is_some(),
            formats: info.formats.iter().map(|f| f.as_str().into()).collect(),
        }
    }
}

/// 账号。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    /// 主键。
    pub id: String,
    /// 游戏。
    pub game: String,
    /// UID。
    pub uid: String,
    /// 服务器。
    pub region: Option<String>,
    /// 备注名。
    pub nickname: Option<String>,
    /// 服务器时区偏移（分钟），记录时间按它显示。
    pub tz_offset_min: i32,
    /// 创建时间。
    pub created_at: i64,
    /// 记录条数（含免费抽）。
    pub records: u64,
    /// 最近一次导入时间。
    pub last_import_at: Option<i64>,
}

impl AccountDto {
    /// 由账号与活动信息组成。
    pub fn new(a: Account, records: u64, last_import_at: Option<i64>) -> Self {
        Self {
            id: a.id.to_string(),
            game: a.game.to_string(),
            uid: a.uid,
            region: a.region,
            nickname: a.nickname,
            tz_offset_min: a.tz_offset_min,
            created_at: a.created_at,
            records,
            last_import_at,
        }
    }
}

/// 一只最高星是否 UP。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UpMark {
    /// UP。
    Up,
    /// 歪。
    NotUp,
    /// 缺卡池数据，无法判断。
    Unknown,
}

impl From<UpStatus> for UpMark {
    fn from(s: UpStatus) -> Self {
        match s {
            UpStatus::Up => Self::Up,
            UpStatus::NotUp => Self::NotUp,
            UpStatus::Unknown => Self::Unknown,
        }
    }
}

/// 某星级的数量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RarityCount {
    /// 星级。
    pub rarity: u8,
    /// 数量。
    pub count: u32,
}

fn rarity_counts(m: &BTreeMap<u8, u32>) -> Vec<RarityCount> {
    m.iter()
        .rev()
        .map(|(&rarity, &count)| RarityCount { rarity, count })
        .collect()
}

/// 一只最高星（出金历史）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TopPullDto {
    /// 物品名。
    pub item_name: String,
    /// 时间。
    pub pulled_at: i64,
    /// 花费抽数。
    pub pulls: u32,
    /// 是否 UP；常驻池为空。
    pub up: Option<UpMark>,
    /// UP 判定为估算。
    pub estimated: bool,
    /// 出这只时处于大保底。
    pub was_guaranteed: bool,
}

impl TopPullDto {
    /// 换算；`has_up` 为假（常驻池）时不带 UP 标注。
    pub fn new(t: &TopPull, has_up: bool) -> Self {
        Self {
            item_name: t.item_name.clone(),
            pulled_at: t.pulled_at,
            pulls: t.pulls,
            up: has_up.then(|| t.up.into()),
            estimated: has_up && t.estimated,
            was_guaranteed: t.was_guaranteed,
        }
    }
}

/// 当前保底状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PityDto {
    /// 已垫抽数。
    pub since_top: u32,
    /// 距硬保底剩余抽数。
    pub remaining_to_hard: u32,
    /// 下一只最高星为大保底。
    pub guaranteed: bool,
    /// 大保底状态为推断。
    pub guarantee_uncertain: bool,
    /// 命定值（估算）。
    pub path_points: Option<u8>,
    /// 距上次 UP 的抽数。
    pub since_up: Option<u32>,
    /// 当期卡池已抽数（井）。
    pub banner_pulls: u32,
}

/// 歪率。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LossDto {
    /// 歪的次数。
    pub lost: u32,
    /// 小保底状态下出最高星的次数。
    pub total: u32,
    /// 歪率；分母为 0 时为空。
    pub rate: Option<f64>,
}

impl From<LossRate> for LossDto {
    fn from(l: LossRate) -> Self {
        Self {
            lost: l.lost,
            total: l.total,
            rate: l.rate(),
        }
    }
}

/// 一个卡池类型的统计。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PoolStatsDto {
    /// 卡池类型。
    pub pool_type: String,
    /// 总抽数。
    pub total_pulls: u32,
    /// 不计抽数的条目（免费抽、赠送）。
    pub free_count: u32,
    /// 各星级数量，从高到低。
    pub rarity_counts: Vec<RarityCount>,
    /// 最高星，按时间顺序。
    pub tops: Vec<TopPullDto>,
    /// 平均出最高星抽数。
    pub avg_pulls_per_top: Option<f64>,
    /// UP 数。
    pub up_count: u32,
    /// 歪的最高星数。
    pub lost_count: u32,
    /// 平均出 UP 抽数。
    pub avg_pulls_per_up: Option<f64>,
    /// 歪率。
    pub loss: Option<LossDto>,
    /// UP 未知的最高星数。
    pub unknown_count: u32,
    /// UP 判定为估算的最高星数。
    pub estimated_count: u32,
    /// 当前保底。
    pub pity: PityDto,
    /// 最早一条记录。
    pub first_at: Option<i64>,
    /// 最近一条记录。
    pub last_at: Option<i64>,
    /// 下一抽出最高星的概率。
    pub next_top: f64,
    /// 下一抽出 UP 的概率；常驻池为空。
    pub next_up: Option<f64>,
    /// 欧非评价；还没有出过最高星时为空。
    pub luck: Option<LuckDto>,
    /// 成就徽章；还没有出过最高星时为空。
    pub achievements: Vec<AchievementDto>,
}

impl PoolStatsDto {
    fn new(
        p: &PoolStats,
        rule: Option<&gachanote_core::rules::PoolRule>,
        range: (Option<i64>, Option<i64>),
    ) -> Self {
        let has_up = rule.is_some_and(|r| r.up.has_up());
        let next = rule.map(|r| predict(r, &p.pity, 1).at(1));
        Self {
            pool_type: p.pool_type.to_string(),
            total_pulls: p.total_pulls,
            free_count: p.free_count,
            rarity_counts: rarity_counts(&p.rarity_counts),
            tops: p.tops.iter().map(|t| TopPullDto::new(t, has_up)).collect(),
            avg_pulls_per_top: p.avg_pulls_per_top,
            up_count: p.up_count,
            lost_count: if has_up {
                count_u32(p.tops.iter().filter(|t| t.up == UpStatus::NotUp))
            } else {
                0
            },
            avg_pulls_per_up: p.avg_pulls_per_up,
            loss: p.loss.map(Into::into),
            unknown_count: p.unknown_count,
            estimated_count: p.estimated_count,
            pity: PityDto {
                since_top: p.pity.since_top,
                remaining_to_hard: p.pity.remaining_to_hard,
                guaranteed: p.pity.guaranteed,
                guarantee_uncertain: p.pity.guarantee_uncertain,
                path_points: p.pity.path_points,
                since_up: p.pity.since_up,
                banner_pulls: p.pity.banner_pulls,
            },
            first_at: range.0,
            last_at: range.1,
            next_top: next.map_or(0.0, |c| c.top),
            next_up: next.and_then(|c| c.up),
            luck: rule.and_then(|r| luck(r, p)).map(Into::into),
            achievements: rule
                .map(|r| achievements(r, p).into_iter().map(Into::into).collect())
                .unwrap_or_default(),
        }
    }
}

fn count_u32<I: Iterator>(it: I) -> u32 {
    u32::try_from(it.count()).unwrap_or(u32::MAX)
}

/// 一个账号的统计（统计页与总览卡片）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatsDto {
    /// 账号。
    pub account_id: String,
    /// 各卡池类型，顺序同游戏的卡池列表。
    pub pools: Vec<PoolStatsDto>,
    /// 总抽数（全部卡池）。
    pub total_pulls: u32,
    /// 各星级数量（全部卡池），从高到低。
    pub rarity_counts: Vec<RarityCount>,
    /// 最高星数。
    pub top_count: u32,
    /// 平均出最高星抽数（全部卡池）。
    pub avg_pulls_per_top: Option<f64>,
    /// UP 数（有 UP 的卡池）。
    pub up_count: u32,
    /// 歪的最高星数。
    pub lost_count: u32,
    /// 常驻等没有 UP 的卡池里出的最高星数。
    pub standard_top_count: u32,
    /// 平均出 UP 抽数（有 UP 的卡池合计）。
    pub avg_pulls_per_up: Option<f64>,
    /// 歪率（有 UP 的卡池合计）。
    pub loss: Option<LossDto>,
    /// 有任一 UP 判定为估算或未知。
    pub has_estimate: bool,
    /// 没有规则、未参与统计的记录数。
    pub unrated: u32,
    /// 最早一条记录。
    pub first_at: Option<i64>,
    /// 最近一条记录。
    pub last_at: Option<i64>,
}

impl AccountStatsDto {
    /// 汇总。`records` 为该账号的全部记录（取每个卡池的时间范围）。
    pub fn new(
        account_id: &str,
        plugin: &dyn GamePlugin,
        stats: &AccountStats,
        records: &[GachaRecord],
    ) -> Self {
        let mut ranges: BTreeMap<&str, (i64, i64)> = BTreeMap::new();
        for r in records {
            let e = ranges
                .entry(r.data.pool_type.as_str())
                .or_insert((r.data.pulled_at, r.data.pulled_at));
            e.0 = e.0.min(r.data.pulled_at);
            e.1 = e.1.max(r.data.pulled_at);
        }
        let rule = |p: &PoolStats| rule_at(plugin.rules(), &p.pool_type, None);
        let has_up = |p: &PoolStats| rule(p).is_some_and(|r| r.up.has_up());
        let pools: Vec<PoolStatsDto> = stats
            .pools
            .iter()
            .map(|p| {
                let range = ranges
                    .get(p.pool_type.as_str())
                    .map_or((None, None), |&(a, b)| (Some(a), Some(b)));
                PoolStatsDto::new(p, rule(p), range)
            })
            .collect();
        let top_count: u32 = pools.iter().map(|p| count_u32(p.tops.iter())).sum();
        let top_pulls: u64 = pools
            .iter()
            .flat_map(|p| p.tops.iter())
            .map(|t| u64::from(t.pulls))
            .sum();
        let up_pools: Vec<&PoolStatsDto> = stats
            .pools
            .iter()
            .zip(&pools)
            .filter(|(raw, _)| has_up(raw))
            .map(|(_, dto)| dto)
            .collect();
        let up_count: u32 = up_pools.iter().map(|p| p.up_count).sum();
        let up_pulls: f64 = up_pools
            .iter()
            .filter_map(|p| p.avg_pulls_per_up.map(|a| a * f64::from(p.up_count)))
            .sum();
        let loss = up_pools
            .iter()
            .filter_map(|p| p.loss)
            .fold(None, |acc: Option<LossRate>, l| {
                let mut a = acc.unwrap_or_default();
                a.lost += l.lost;
                a.total += l.total;
                Some(a)
            });
        Self {
            account_id: account_id.into(),
            total_pulls: stats.total_pulls,
            rarity_counts: rarity_counts(&stats.rarity_counts),
            top_count,
            avg_pulls_per_top: (top_count > 0).then(|| top_pulls as f64 / f64::from(top_count)),
            up_count,
            lost_count: up_pools.iter().map(|p| p.lost_count).sum(),
            standard_top_count: stats
                .pools
                .iter()
                .zip(&pools)
                .filter(|(raw, _)| !has_up(raw))
                .map(|(_, dto)| count_u32(dto.tops.iter()))
                .sum(),
            avg_pulls_per_up: (up_count > 0).then(|| up_pulls / f64::from(up_count)),
            loss: loss.map(Into::into),
            has_estimate: up_pools
                .iter()
                .any(|p| p.estimated_count > 0 || p.unknown_count > 0),
            unrated: stats.unrated,
            first_at: records.iter().map(|r| r.data.pulled_at).min(),
            last_at: records.iter().map(|r| r.data.pulled_at).max(),
            pools,
        }
    }
}

/// 总览中的一个账号。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OverviewEntry {
    /// 账号。
    pub account: AccountDto,
    /// 统计。
    pub stats: AccountStatsDto,
}

/// 欧非评价口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LuckMetricDto {
    /// 平均出 UP 抽数。
    PerUp,
    /// 平均出最高星抽数。
    PerTop,
}

/// 欧非评价（F-17）。评级文案由前端按百分位分档。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LuckDto {
    /// 口径。
    pub metric: LuckMetricDto,
    /// 样本数。
    pub samples: u32,
    /// 你的平均抽数。
    pub user_avg: f64,
    /// 理论平均抽数。
    pub expected_avg: f64,
    /// 比多少比例的人运气好（0–1）；最高星少于 5 只时为空。
    pub percentile: Option<f64>,
}

impl From<Luck> for LuckDto {
    fn from(l: Luck) -> Self {
        Self {
            metric: match l.metric {
                LuckMetric::PerUp => LuckMetricDto::PerUp,
                LuckMetric::PerTop => LuckMetricDto::PerTop,
            },
            samples: l.samples,
            user_avg: l.user_avg,
            expected_avg: l.expected_avg,
            percentile: l.percentile,
        }
    }
}

/// 成就种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AchievementKindDto {
    /// 欧皇时刻。
    Lucky,
    /// 非酋时刻。
    Unlucky,
    /// 不歪连胜。
    Streak,
    /// 漫长等待。
    LongWait,
}

/// 成就徽章。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AchievementDto {
    /// 种类。
    pub kind: AchievementKindDto,
    /// 是否达成。
    pub done: bool,
    /// 当前最好成绩。
    pub value: u32,
    /// 达成条件。
    pub threshold: u32,
    /// 对应物品名。
    pub item_name: Option<String>,
}

impl From<Achievement> for AchievementDto {
    fn from(a: Achievement) -> Self {
        Self {
            kind: match a.kind {
                AchievementKind::Lucky => AchievementKindDto::Lucky,
                AchievementKind::Unlucky => AchievementKindDto::Unlucky,
                AchievementKind::Streak => AchievementKindDto::Streak,
                AchievementKind::LongWait => AchievementKindDto::LongWait,
            },
            done: a.done,
            value: a.value,
            threshold: a.threshold,
            item_name: a.item_name,
        }
    }
}

/// 概率曲线上的一点。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CurvePointDto {
    /// 再抽多少抽。
    pub pulls: u32,
    /// 至少出一只最高星的概率。
    pub top: f64,
    /// 至少出一只 UP 的概率；常驻池为空。
    pub up: Option<f64>,
}

/// 出金 / 出 UP 概率预测（F-16）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PredictionDto {
    /// 卡池类型。
    pub pool_type: String,
    /// 当前已垫抽数。
    pub since_top: u32,
    /// 下一只最高星是否大保底。
    pub guaranteed: bool,
    /// 累计概率曲线，`points[k]` 为再抽 k 抽内。
    pub points: Vec<CurvePointDto>,
    /// 出最高星累计概率达到 50% / 90% 的抽数。
    pub top_q50: Option<u32>,
    /// 同上，90%。
    pub top_q90: Option<u32>,
    /// 出 UP 累计概率达到 50% 的抽数。
    pub up_q50: Option<u32>,
    /// 同上，90%。
    pub up_q90: Option<u32>,
    /// 最多再抽几抽必出最高星。
    pub top_guaranteed_within: u32,
    /// 最多再抽几抽必得 UP；没有这种保证时为空。
    pub up_guaranteed_within: Option<u32>,
}

impl PredictionDto {
    /// 由预测结果组成。
    pub fn new(pool_type: &str, pity: &gachanote_core::stats::PityStatus, p: &Prediction) -> Self {
        Self {
            pool_type: pool_type.into(),
            since_top: pity.since_top,
            guaranteed: pity.guaranteed,
            points: p
                .points
                .iter()
                .map(|c| CurvePointDto {
                    pulls: c.pulls,
                    top: c.top,
                    up: c.up,
                })
                .collect(),
            top_q50: p.quantile(0.5, false),
            top_q90: p.quantile(0.9, false),
            up_q50: p.quantile(0.5, true),
            up_q90: p.quantile(0.9, true),
            top_guaranteed_within: p.top_guaranteed_within,
            up_guaranteed_within: p.up_guaranteed_within,
        }
    }
}

/// 一个月的抽卡情况（F-15）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MonthDto {
    /// 年月，如 `2026-10`（服务器时区）。
    pub month: String,
    /// 抽数。
    pub pulls: u32,
    /// 最高星数。
    pub tops: u32,
    /// 其中 UP 数。
    pub ups: u32,
}

impl From<MonthBucket> for MonthDto {
    fn from(m: MonthBucket) -> Self {
        Self {
            month: m.month,
            pulls: m.pulls,
            tops: m.tops,
            ups: m.ups,
        }
    }
}

/// 按日抽数（时间线图表，F-19）：从 `start` 起每天一项，三个数组等长。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DailyDto {
    /// 第一天（服务器时区），如 `2026-10-01`；没有记录时为空串。
    pub start: String,
    /// 每天的最高星数。
    pub top: Vec<u32>,
    /// 每天的次高星数。
    pub second: Vec<u32>,
    /// 每天的其他星级数。
    pub other: Vec<u32>,
}

/// 物品的各语言名称（卡池 UP 名单用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ItemNameDto {
    /// 元数据包里的写法（物品 ID 或名称），找不到物品时直接显示它。
    pub key: String,
    /// 各语言名称，键为 `zh-cn`、`en-us` 等。
    pub names: BTreeMap<String, String>,
}

/// 时间线上的一期是怎么划分的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PeriodKind {
    /// 元数据包里的一期卡池。
    Banner,
    /// 记录自带的卡池 ID。
    Pool,
    /// 按时间自动分段。
    Auto,
}

impl From<PeriodSource> for PeriodKind {
    fn from(s: PeriodSource) -> Self {
        match s {
            PeriodSource::Banner => Self::Banner,
            PeriodSource::Pool => Self::Pool,
            PeriodSource::Auto => Self::Auto,
        }
    }
}

/// 时间线上的一期（F-19）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PeriodDto {
    /// 划分方式。
    pub kind: PeriodKind,
    /// 卡池 ID。
    pub id: Option<String>,
    /// 卡池名：元数据包里的各语言名称。
    pub names: BTreeMap<String, String>,
    /// 卡池名：记录原文（方舟、终末地记录自带）。
    pub raw_name: Option<String>,
    /// 开始时间。
    pub start: i64,
    /// 结束时间。
    pub end: i64,
    /// UP 最高星。
    pub up_top: Vec<ItemNameDto>,
    /// UP 次高星。
    pub up_second: Vec<ItemNameDto>,
    /// 本期抽数；为 0 表示未参与。
    pub pulls: u32,
    /// 本期出的最高星，按时间顺序。
    pub tops: Vec<TopPullDto>,
}

/// 一个卡池的时间线：按日图表与各期列表（最新在前）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimelineDto {
    /// 卡池类型。
    pub pool_type: String,
    /// 按日抽数。
    pub daily: DailyDto,
    /// 各期。
    pub periods: Vec<PeriodDto>,
}

/// 鹰角 token 的来源。token 本身只在「手动粘贴」时经过前端。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TokenSource {
    /// 登录窗口取得的 token（`hg_login` 返回的编号）。
    Login {
        /// 编号。
        id: String,
    },
    /// 用户粘贴的 token（或官网返回的整段 JSON）。
    Pasted {
        /// 内容。
        token: String,
    },
    /// 某账号已保存的 token。
    Saved {
        /// 账号。
        #[serde(rename = "accountId")]
        account_id: String,
    },
}

/// 鹰角账号下的一个游戏角色。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HgRoleDto {
    /// 游戏。
    pub game: String,
    /// UID。
    pub uid: String,
    /// 游戏内昵称。
    pub nickname: Option<String>,
    /// 渠道（官服 / B 服）。
    pub channel: Option<String>,
    /// 本地是否已有该账号。
    pub exists: bool,
}

/// 一条已保存的凭证（不含内容）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialDto {
    /// 账号。
    pub account_id: String,
    /// 游戏。
    pub game: String,
    /// UID。
    pub uid: String,
    /// 备注名。
    pub nickname: Option<String>,
    /// 种类（`hg_account_token`）。
    pub kind: String,
    /// 保存时间。
    pub updated_at: i64,
}

/// 可以用系统浏览器打开的外部页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExternalPage {
    /// 鹰角官网登录页（手动获取 token 第 1 步）。
    HgLogin,
    /// 鹰角 token 查询页（第 2 步）。
    HgToken,
    /// GitHub 发布页（自动更新不可用时手动下载）。
    Releases,
}

impl ExternalPage {
    /// 网址。
    pub fn url(self) -> &'static str {
        match self {
            Self::HgLogin => gachanote_games::hypergryph::LOGIN_URL,
            Self::HgToken => gachanote_games::hypergryph::TOKEN_URL,
            Self::Releases => "https://github.com/zchasing/GachaNote/releases",
        }
    }
}

/// 新版本信息（F-24）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 新版本号。
    pub version: String,
    /// 当前版本号。
    pub current_version: String,
    /// 发布时间（RFC 3339）。
    pub date: Option<String>,
    /// 更新说明（Markdown）。
    pub notes: Option<String>,
}

/// 下载更新的进度（`update://progress`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "update://progress")]
pub struct UpdateProgress {
    /// 已下载字节数。
    pub downloaded: u64,
    /// 总字节数（服务器没给时为空）。
    pub total: Option<u64>,
}

/// 记录列表查询。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordQueryDto {
    /// 账号。
    pub account_id: String,
    /// 卡池类型，空表示全部。
    pub pool_types: Vec<String>,
    /// 星级，空表示全部。
    pub rarities: Vec<u8>,
    /// 起始时间（含）。
    pub from: Option<i64>,
    /// 结束时间（不含）。
    pub to: Option<i64>,
    /// 物品名搜索。
    pub name: Option<String>,
    /// 页码，从 0 开始。
    pub page: u32,
    /// 每页条数。
    pub page_size: u32,
}

/// 记录列表中的一条。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordDto {
    /// 主键。
    pub id: String,
    /// 卡池类型。
    pub pool_type: String,
    /// 物品名。
    pub item_name: String,
    /// 物品 ID（查头像用）。
    pub item_id: Option<String>,
    /// 物品类型。
    pub item_type: Option<String>,
    /// 星级。
    pub rarity: u8,
    /// 时间。
    pub pulled_at: i64,
    /// 是否计入抽数。
    pub is_pull: bool,
    /// 当次抽数（仅最高星与次高星）。
    pub pulls: Option<u32>,
    /// 是否 UP（仅有 UP 的卡池的最高星）。
    pub up: Option<UpMark>,
    /// UP 判定为估算。
    pub estimated: bool,
}

impl RecordDto {
    /// 由记录与标注组成。
    pub fn new(r: GachaRecord, mark: RecordMark) -> Self {
        Self {
            id: r.id.to_string(),
            pool_type: r.data.pool_type.to_string(),
            item_name: r.data.item_name,
            item_id: r.data.item_id,
            item_type: r.data.item_type,
            rarity: r.data.rarity,
            pulled_at: r.data.pulled_at,
            is_pull: r.data.is_pull,
            pulls: mark.pulls,
            up: mark.up.map(Into::into),
            estimated: mark.estimated,
        }
    }
}

/// 一页记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecordPage {
    /// 本页。
    pub items: Vec<RecordDto>,
    /// 满足条件的总条数。
    pub total: u64,
    /// 页码。
    pub page: u32,
    /// 每页条数。
    pub page_size: u32,
}

/// 文件预览中的一个账号。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAccountDto {
    /// 游戏。
    pub game: String,
    /// UID。
    pub uid: String,
    /// 记录条数。
    pub count: u32,
    /// 解码失败的条数。
    pub failed: u32,
    /// 最早记录。
    pub from: Option<i64>,
    /// 最近记录。
    pub to: Option<i64>,
    /// 本地是否已有该账号。
    pub exists: bool,
    /// 本软件是否支持这款游戏。
    pub supported: bool,
}

/// 文件预览。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FilePreviewDto {
    /// 文件路径。
    pub path: String,
    /// 文件名。
    pub file_name: String,
    /// 识别出的格式。
    pub format: String,
    /// 文件里的账号。
    pub accounts: Vec<PreviewAccountDto>,
}

impl FilePreviewDto {
    /// 由预览组成；`exists` 判断账号是否已在本地。
    pub fn new(
        path: &std::path::Path,
        p: FilePreview,
        exists: impl Fn(&str, &str) -> bool,
        supported: impl Fn(&str) -> bool,
    ) -> Self {
        Self {
            path: path.display().to_string(),
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            format: p.format.as_str().into(),
            accounts: p
                .accounts
                .into_iter()
                .map(|a| PreviewAccountDto {
                    exists: exists(a.game.as_str(), &a.uid),
                    supported: supported(a.game.as_str()),
                    game: a.game.to_string(),
                    uid: a.uid,
                    count: a.count,
                    failed: a.failed,
                    from: a.from,
                    to: a.to,
                })
                .collect(),
        }
    }
}

/// `(游戏, UID)`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountKey {
    /// 游戏。
    pub game: String,
    /// UID。
    pub uid: String,
}

/// 导入报告中的一个账号。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountReportDto {
    /// 账号。
    pub account_id: String,
    /// 游戏。
    pub game: String,
    /// UID。
    pub uid: String,
    /// 本次新建。
    pub created: bool,
    /// 新增。
    pub added: u32,
    /// 重复跳过。
    pub skipped: u32,
    /// 失败。
    pub failed: u32,
}

/// 一条失败记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FailureDto {
    /// 游戏。
    pub game: String,
    /// UID。
    pub uid: String,
    /// 位置（行号或 JSON 路径）。
    pub location: String,
    /// 原因。
    pub reason: String,
}

/// 导入报告（FR-IMP-07）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReportDto {
    /// 各账号。
    pub accounts: Vec<AccountReportDto>,
    /// 失败记录（最多 200 条，其余只计数）。
    pub failures: Vec<FailureDto>,
    /// 新增合计。
    pub added: u32,
    /// 跳过合计。
    pub skipped: u32,
    /// 失败合计。
    pub failed: u32,
    /// 写入前的备份文件名。
    pub backup: Option<String>,
}

/// 报告里最多带多少条失败明细。
const MAX_FAILURES: usize = 200;

impl From<&ImportReport> for ImportReportDto {
    fn from(r: &ImportReport) -> Self {
        Self {
            accounts: r
                .accounts
                .iter()
                .map(|a| AccountReportDto {
                    account_id: a.account_id.to_string(),
                    game: a.game.to_string(),
                    uid: a.uid.clone(),
                    created: a.created,
                    added: a.added,
                    skipped: a.skipped,
                    failed: a.failed,
                })
                .collect(),
            failures: r
                .failures
                .iter()
                .take(MAX_FAILURES)
                .map(|f| FailureDto {
                    game: f.game.to_string(),
                    uid: f.uid.clone(),
                    location: f.location.clone(),
                    reason: f.reason.clone(),
                })
                .collect(),
            added: r.added(),
            skipped: r.skipped(),
            failed: r.failed(),
            backup: r.backup.clone(),
        }
    }
}

/// 自动找到的抽卡链接。链接本身留在后端，前端只拿到编号与脱敏后的地址。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredUrl {
    /// 编号，拉取时传回。
    pub source_id: String,
    /// 游戏。
    pub game: String,
    /// 脱敏后的地址（只有域名和路径）。
    pub display: String,
}

/// 拉取用的链接来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UrlSource {
    /// `discover_url` 找到的链接。
    Discovered {
        /// 编号。
        id: String,
    },
    /// 用户粘贴的链接。
    Pasted {
        /// 链接。
        url: String,
    },
}

/// 拉取模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum FetchModeDto {
    /// 增量。
    Incremental,
    /// 全量。
    Full,
}

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    /// UIGF v4.2（米哈游三款）。
    Uigf,
    /// GachaNote JSON。
    Gnj,
    /// CSV。
    Csv,
}

/// 导出结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// 写入的文件。
    pub path: String,
    /// 账号数。
    pub accounts: u32,
    /// 记录数。
    pub records: u64,
}

/// 用户设置（存在数据库 `setting` 表）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 界面语言：`system` / `zh-CN` / `en`。
    pub language: String,
    /// 主题：`system` / `light` / `dark`。
    pub theme: String,
    /// 自定义代理（如 `http://127.0.0.1:7890`），空表示跟随系统；重启后生效。
    pub proxy: Option<String>,
    /// 用户在边栏手动添加的游戏（没有账号也显示）。
    pub sidebar_games: Vec<String>,
    /// 每款游戏上次查看的账号。
    pub last_accounts: BTreeMap<String, String>,
    /// 启动时检查卡池数据包更新。
    pub meta_auto_update: bool,
    /// 启动时检查软件新版本。
    pub update_auto_check: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            theme: "system".into(),
            proxy: None,
            sidebar_games: Vec::new(),
            last_accounts: BTreeMap::new(),
            meta_auto_update: true,
            update_auto_check: true,
        }
    }
}

/// 一份备份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupDto {
    /// 文件名。
    pub name: String,
    /// 创建时间。
    pub created_at: i64,
    /// 大小（字节）。
    pub size: u64,
}

/// 卡池数据包状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MetaInfo {
    /// 版本。
    pub version: String,
    /// 来源：`bundled`（内置）/ `cache`（已下载）。
    pub origin: String,
}

/// 检查卡池数据包更新的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MetaUpdateResult {
    /// `updated` / `upToDate` / `appTooOld`。
    pub outcome: String,
    /// 版本。
    pub version: String,
    /// 远程包需要的应用版本（`appTooOld` 时）。
    pub min_app_version: Option<String>,
}

/// 应用与目录信息（设置页「关于」「数据」）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// 应用版本。
    pub version: String,
    /// 数据目录（数据库与备份）。
    pub data_dir: String,
    /// 默认数据目录。
    pub default_data_dir: String,
    /// 日志目录。
    pub log_dir: String,
    /// 数据库文件大小（字节）。
    pub db_size: u64,
    /// 是否在桌面端能自动查找游戏链接（Windows）。
    pub can_discover: bool,
    /// 能否加密保存 token（Windows DPAPI）。
    pub can_remember_token: bool,
}

/// `open_dir` 可以打开的目录。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DirKind {
    /// 数据目录。
    Data,
    /// 备份目录。
    Backups,
    /// 日志目录。
    Logs,
}

/// 任务阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum StageDto {
    /// 查找链接。
    Discover,
    /// 拉取。
    Fetch,
    /// 解析。
    Parse,
    /// 备份。
    Backup,
    /// 写入。
    Write,
}

/// 任务进度（`task://progress`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "task://progress")]
pub struct TaskProgress {
    /// 任务。
    pub task_id: u64,
    /// 阶段。
    pub stage: StageDto,
    /// 当前进度。
    pub current: u64,
    /// 总量。
    pub total: Option<u64>,
    /// 说明（如当前卡池代码）。
    pub message: String,
}

impl TaskProgress {
    /// 由核心层的进度组成。
    pub fn new(task_id: u64, e: ProgressEvent) -> Self {
        Self {
            task_id,
            stage: match e.stage {
                Stage::Discover => StageDto::Discover,
                Stage::Fetch => StageDto::Fetch,
                Stage::Parse => StageDto::Parse,
                Stage::Backup => StageDto::Backup,
                Stage::Write => StageDto::Write,
            },
            current: e.current,
            total: e.total,
            message: e.message,
        }
    }
}

/// 任务完成（`task://done`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "task://done")]
pub struct TaskDone {
    /// 任务。
    pub task_id: u64,
    /// 导入报告。
    pub report: ImportReportDto,
}

/// 任务失败（`task://failed`），数据未写入。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "task://failed")]
pub struct TaskFailed {
    /// 任务。
    pub task_id: u64,
    /// 错误。
    pub error: ErrorDto,
}

/// 数据有写入（`data://changed`）：前端据此刷新统计与边栏。`account_ids` 为空表示全部。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "data://changed")]
pub struct DataChanged {
    /// 受影响的账号。
    pub account_ids: Vec<String>,
}

/// 卡池数据包已更新（`meta://updated`），UP 判定会重新计算。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "meta://updated")]
pub struct MetaUpdated {
    /// 新版本。
    pub version: String,
}
