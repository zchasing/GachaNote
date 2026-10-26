//! 规则模板：每种卡池类型一份，描述保底曲线、UP 规则与特殊机制（架构文档 3.1）。
//!
//! 规则是纯数据，可以写死在插件里，也可以被元数据包覆盖。游戏调整参数时，
//! 用 [`PoolRule::since`] 按生效日期分段，统计时按记录时间选用当时的规则。

use serde::{Deserialize, Serialize};

use crate::ids::PoolTypeId;

/// 单抽最高星概率曲线：基础概率 + 软保底线性递增 + 硬保底必出（SRS 第 5 节）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PityCurve {
    /// 基础概率 p0。
    pub base: f64,
    /// 软保底起点 s（第几抽开始递增）；`None` 表示没有软保底。
    pub soft_start: Option<u32>,
    /// 每抽增量 d。
    pub step: f64,
    /// 硬保底 h：第 h 抽必出。
    pub hard: u32,
}

impl PityCurve {
    /// 距上次最高星第 `k` 抽（从 1 开始）出最高星的概率。
    pub fn probability(&self, k: u32) -> f64 {
        if k >= self.hard {
            return 1.0;
        }
        match self.soft_start {
            Some(s) if k >= s => (self.base + f64::from(k - s + 1) * self.step).min(1.0),
            _ => self.base,
        }
    }
}

/// UP 规则。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpRule {
    /// 常驻池，没有 UP。
    None,
    /// 大小保底：出最高星时 `rate` 概率为 UP；`guarantee_next` 为真时歪了之后下一只必为 UP。
    FiftyFifty {
        /// 小保底时为 UP 的概率。
        rate: f64,
        /// 歪了之后是否进入大保底。
        guarantee_next: bool,
    },
    /// 固定概率为 UP，没有大小保底（明日方舟）。
    Fixed {
        /// 为 UP 的概率。
        rate: f64,
    },
}

impl UpRule {
    /// 是否有 UP（即是否统计歪率与平均出 UP 抽数）。
    pub fn has_up(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// 可叠加的特殊机制。新增游戏时若不够用，新增变体并在统计引擎里实现其状态转移。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mechanic {
    /// 原神武器池「神铸定轨」：命定值满 `max` 时下一只最高星必为定轨物品；换期清零。
    EpitomizedPath {
        /// 命定值上限。
        max: u8,
    },
    /// `n` 抽内必出 UP。`per_banner` 为真时进度换期清零（终末地 120 抽）；
    /// 为假时跨期累计、触发一次后失效（方舟登场限定 150 抽）。
    UpWithinN {
        /// 抽数。
        n: u32,
        /// 是否换期清零。
        per_banner: bool,
    },
    /// 当期累计 `n` 抽可兑换 / 自选（方舟限定池 300 抽、崩铁常驻 300 抽）。
    Spark {
        /// 抽数。
        n: u32,
    },
    /// 有不计入保底的免费抽（终末地）；具体条目由记录的 `is_pull = false` 标出。
    FreePulls,
    /// 预留：原神「捕获明光」，只影响概率预测，不影响记录扫描。
    CapturingRadiance,
}

/// 保底进度的继承范围。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PityScope {
    /// 同一卡池类型（保底组）内跨期继承。
    #[default]
    SharedByType,
    /// 每期卡池重置（方舟季节限定等）。
    PerBanner,
}

/// 一种卡池类型的规则模板。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolRule {
    /// 卡池类型（保底共享组）。
    pub pool_type: PoolTypeId,
    /// 生效起始时间（UTC 毫秒），`None` 表示一直有效。同一类型有多份时取不晚于记录时间的最新一份。
    #[serde(default)]
    pub since: Option<i64>,
    /// 最高星级（5 或 6）。
    pub rarity_top: u8,
    /// 单抽概率曲线。
    pub curve: PityCurve,
    /// UP 规则。
    pub up: UpRule,
    /// 特殊机制。
    #[serde(default)]
    pub extras: Vec<Mechanic>,
    /// 保底继承范围。
    #[serde(default)]
    pub pity_scope: PityScope,
}

impl PoolRule {
    /// 命定值上限（没有定轨机制时为 `None`）。
    pub fn path_max(&self) -> Option<u8> {
        self.extras.iter().find_map(|m| match m {
            Mechanic::EpitomizedPath { max } => Some(*max),
            _ => None,
        })
    }

    /// 「N 抽必出 UP」机制。
    pub fn up_within(&self) -> Option<(u32, bool)> {
        self.extras.iter().find_map(|m| match m {
            Mechanic::UpWithinN { n, per_banner } => Some((*n, *per_banner)),
            _ => None,
        })
    }

    /// 井 / 自选所需抽数。
    pub fn spark(&self) -> Option<u32> {
        self.extras.iter().find_map(|m| match m {
            Mechanic::Spark { n } => Some(*n),
            _ => None,
        })
    }
}

/// 从一组规则中选出 `pool_type` 在时间 `at` 生效的那一份；`at = None` 取最新一份。
pub fn rule_at<'a>(
    rules: &'a [PoolRule],
    pool_type: &PoolTypeId,
    at: Option<i64>,
) -> Option<&'a PoolRule> {
    rules
        .iter()
        .filter(|r| &r.pool_type == pool_type)
        .filter(|r| match (r.since, at) {
            (Some(since), Some(at)) => since <= at,
            _ => true,
        })
        .max_by_key(|r| r.since.unwrap_or(i64::MIN))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genshin_curve() -> PityCurve {
        PityCurve {
            base: 0.006,
            soft_start: Some(74),
            step: 0.06,
            hard: 90,
        }
    }

    #[test]
    fn curve_three_segments() {
        let c = genshin_curve();
        assert_eq!(c.probability(1), 0.006);
        assert_eq!(c.probability(73), 0.006);
        assert!((c.probability(74) - 0.066).abs() < 1e-12);
        assert!((c.probability(75) - 0.126).abs() < 1e-12);
        assert!((c.probability(89) - 0.966).abs() < 1e-12);
        assert_eq!(c.probability(90), 1.0);
        assert_eq!(c.probability(200), 1.0);
        let flat = PityCurve {
            soft_start: None,
            ..c
        };
        assert_eq!(flat.probability(80), 0.006);
    }

    fn rule(since: Option<i64>, max: u8) -> PoolRule {
        PoolRule {
            pool_type: "weapon".into(),
            since,
            rarity_top: 5,
            curve: genshin_curve(),
            up: UpRule::FiftyFifty {
                rate: 0.75,
                guarantee_next: true,
            },
            extras: vec![
                Mechanic::EpitomizedPath { max },
                Mechanic::UpWithinN {
                    n: 120,
                    per_banner: true,
                },
                Mechanic::Spark { n: 300 },
            ],
            pity_scope: PityScope::SharedByType,
        }
    }

    #[test]
    fn rule_selection_by_date() {
        let rules = vec![rule(None, 2), rule(Some(1_000), 1)];
        let w = PoolTypeId::from("weapon");
        assert_eq!(rule_at(&rules, &w, Some(999)).unwrap().path_max(), Some(2));
        assert_eq!(
            rule_at(&rules, &w, Some(1_000)).unwrap().path_max(),
            Some(1)
        );
        assert_eq!(rule_at(&rules, &w, None).unwrap().path_max(), Some(1));
        assert!(rule_at(&rules, &"character".into(), None).is_none());
    }

    #[test]
    fn mechanic_accessors() {
        let r = rule(None, 1);
        assert_eq!(r.up_within(), Some((120, true)));
        assert_eq!(r.spark(), Some(300));
        assert!(r.up.has_up());
        assert!(!UpRule::None.has_up());
        let plain = PoolRule {
            extras: vec![Mechanic::FreePulls, Mechanic::CapturingRadiance],
            ..r
        };
        assert_eq!(plain.path_max(), None);
        assert_eq!(plain.up_within(), None);
        assert_eq!(plain.spark(), None);
    }

    #[test]
    fn serde_shape() {
        let json = serde_json::to_value(rule(None, 1)).unwrap();
        assert_eq!(json["up"]["kind"], "fifty_fifty");
        assert_eq!(
            json["extras"][0],
            serde_json::json!({"kind": "epitomized_path", "max": 1})
        );
        assert_eq!(json["pity_scope"], "shared_by_type");
        let back: PoolRule = serde_json::from_value(json).unwrap();
        assert_eq!(back, rule(None, 1));
    }
}
