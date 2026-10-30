//! 出金 / 出 UP 概率预测（架构文档 6.3，F-16）。
//!
//! 从当前保底状态出发，逐抽推演「再抽 k 抽内至少出一只最高星 / 至少出一只 UP」的累计概率。
//!
//! - **出最高星**：P = 1 − ∏(1 − p(m + i))，到硬保底时必出。
//! - **出 UP**：动态规划。出 UP 之前 `since_up` 与当期抽数每抽都加 1，是步数的确定函数，
//!   所以随机状态只有（已垫抽数, 是否大保底, 命定值），状态数最多 90 × 2 × 3，
//!   300 抽以内的查询在毫秒级。
//!
//! 规则模板里会改变出 UP 概率的机制都算进去：
//!
//! | 机制 | 预测时的处理 |
//! | --- | --- |
//! | 大小保底 | 歪了之后下一只最高星必为 UP |
//! | 命定值 | 命定值满时下一只最高星必为 UP（定轨对象按 UP 计） |
//! | N 抽必出 UP，每期清零（终末地 120、方舟联动 120） | 当期第 N 抽仍未出 UP 时，这一抽必出 UP |
//! | N 抽必出 UP，跨期一次性（方舟登场限定 150） | 满 N 抽未出 UP 后，下一只最高星必为 UP |
//! | 井 / 自选 | 当期抽满 N 抽时可直接兑换 UP，视为已得到 |
//!
//! 「捕获明光」等官方未公示细节的机制不计入，界面上按普通大小保底预测。

use serde::{Deserialize, Serialize};

use super::PityStatus;
use crate::rules::{PoolRule, UpRule};

/// 不论请求多少抽，曲线最多算到这么长。
pub const MAX_PULLS: u32 = 1000;
/// 曲线默认至少算到两项概率都超过这个值（或到 [`MAX_PULLS`]）。
const SETTLED: f64 = 0.999;

/// 曲线上的一点：再抽 `pulls` 抽内的累计概率。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    /// 再抽多少抽。
    pub pulls: u32,
    /// 至少出一只最高星的概率。
    pub top: f64,
    /// 至少出一只 UP 的概率；常驻池为空。
    pub up: Option<f64>,
}

/// 预测结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    /// 从 0 抽开始的累计概率曲线，`points[k].pulls == k`。
    pub points: Vec<CurvePoint>,
    /// 最多再抽几抽必出最高星（即距硬保底的剩余抽数）。
    pub top_guaranteed_within: u32,
    /// 最多再抽几抽必得 UP；规则没有这种保证（如方舟标准寻访）时为空。
    pub up_guaranteed_within: Option<u32>,
}

impl Prediction {
    /// 再抽 `k` 抽内的累计概率；超出曲线长度时取最后一点。
    pub fn at(&self, k: u32) -> CurvePoint {
        let i = usize::try_from(k).unwrap_or(usize::MAX);
        self.points
            .get(i)
            .or(self.points.last())
            .copied()
            .unwrap_or(CurvePoint {
                pulls: 0,
                top: 0.0,
                up: None,
            })
    }

    /// 累计概率首次达到 `p` 的抽数（如 50% / 90% 分位）；曲线内达不到时为空。
    pub fn quantile(&self, p: f64, up: bool) -> Option<u32> {
        self.points
            .iter()
            .find(|c| {
                let v = if up { c.up } else { Some(c.top) };
                v.is_some_and(|v| v >= p - 1e-12)
            })
            .map(|c| c.pulls)
    }
}

/// 推演用的静态参数（取自规则模板与当前保底状态）。
struct Model<'a> {
    rule: &'a PoolRule,
    hard: u32,
    /// 命定值上限，没有定轨时为 0。
    path_max: u8,
    /// 「N 抽必出 UP」：(N, 是否每期清零)，且当前仍然有效。
    up_within: Option<(u32, bool)>,
    since_up: u32,
    /// 井：当期抽满多少抽可兑换。
    spark: Option<u32>,
    banner_pulls: u32,
}

impl Model<'_> {
    /// 第 `i` 抽（从 1 开始）是否因「每期 N 抽必出 UP」而必出 UP。
    fn forced_at(&self, i: u32) -> bool {
        matches!(self.up_within, Some((n, true)) if self.since_up + i >= n)
    }

    /// 第 `i` 抽时，出的最高星是否因「满 N 抽后下一只必为 UP」而必为 UP。
    fn next_top_is_up(&self, i: u32) -> bool {
        matches!(self.up_within, Some((n, false)) if self.since_up + i > n)
    }

    /// 第 `i` 抽之后是否可以用井兑换 UP。
    fn sparked_at(&self, i: u32) -> bool {
        self.spark.is_some_and(|n| self.banner_pulls + i >= n)
    }
}

/// 从当前保底状态出发预测，曲线至少算到 `pulls` 抽，并延长到两项概率都基本确定。
pub fn predict(rule: &PoolRule, pity: &PityStatus, pulls: u32) -> Prediction {
    let hard = rule.curve.hard.max(1);
    let since_top = pity.since_top.min(hard - 1);
    let has_up = rule.up.has_up();
    let path_max = rule.path_max().unwrap_or(0);
    let since_up = pity.since_up.unwrap_or(0);
    // 每期清零的「N 抽必出 UP」只在当期还没出过 UP 时有效：出过 UP 后 since_up 会小于当期抽数
    let up_within = rule
        .up_within()
        .filter(|&(_, per_banner)| !per_banner || since_up >= pity.banner_pulls);
    let m = Model {
        rule,
        hard,
        path_max,
        up_within: up_within.filter(|_| has_up),
        since_up,
        spark: rule.spark().filter(|_| has_up),
        banner_pulls: pity.banner_pulls,
    };
    let limit = pulls.min(MAX_PULLS);

    // 出最高星：只与已垫抽数有关
    let top_guaranteed_within = hard - since_top;
    // DP 状态：(已垫抽数, 大保底, 命定值) → 尚未出 UP 的概率质量
    let width = usize::from(path_max) + 1;
    let idx = |s: u32, g: bool, pp: u8| -> usize {
        ((s as usize) * 2 + usize::from(g)) * width + usize::from(pp)
    };
    let states = (hard as usize) * 2 * width;
    let mut mass = vec![0.0f64; states];
    let path0 = pity.path_points.unwrap_or(0).min(path_max);
    mass[idx(since_top, pity.guaranteed, path0)] = 1.0;

    let mut points = vec![CurvePoint {
        pulls: 0,
        top: 0.0,
        up: has_up.then_some(0.0),
    }];
    let mut no_top = 1.0f64;
    let mut up_hit = 0.0f64;
    let mut up_guaranteed_within = None;
    let mut next = vec![0.0f64; states];
    let mut i = 0u32;
    loop {
        let done_top = 1.0 - no_top >= SETTLED;
        let done_up = !has_up || up_hit >= SETTLED;
        if (i >= limit && done_top && done_up) || i >= MAX_PULLS {
            break;
        }
        i += 1;
        if m.forced_at(i) {
            // 这一抽必出 UP（也是一只最高星）
            no_top = 0.0;
            up_hit = 1.0;
            mass.iter_mut().for_each(|w| *w = 0.0);
        } else {
            let p_top = rule.curve.probability(since_top + i);
            if since_top + i <= hard {
                no_top *= 1.0 - p_top;
            }
            if has_up {
                up_hit += step(&m, &mut mass, &mut next, i, idx);
                std::mem::swap(&mut mass, &mut next);
            }
        }
        if has_up && m.sparked_at(i) {
            up_hit = 1.0;
            mass.iter_mut().for_each(|w| *w = 0.0);
        }
        let up = has_up.then(|| up_hit.min(1.0));
        if up_guaranteed_within.is_none() && up.is_some_and(|u| u >= 1.0 - 1e-12) {
            up_guaranteed_within = Some(i);
        }
        points.push(CurvePoint {
            pulls: i,
            top: (1.0 - no_top).clamp(0.0, 1.0),
            up,
        });
    }
    if has_up && up_guaranteed_within.is_none() {
        up_guaranteed_within = up_guarantee_bound(&m, since_top, pity, path0);
    }
    Prediction {
        points,
        top_guaranteed_within,
        up_guaranteed_within,
    }
}

/// 推演一抽：把 `mass` 转移到 `next`，返回这一抽新出 UP 的概率。
fn step(
    m: &Model<'_>,
    mass: &mut [f64],
    next: &mut [f64],
    i: u32,
    idx: impl Fn(u32, bool, u8) -> usize,
) -> f64 {
    next.iter_mut().for_each(|w| *w = 0.0);
    let (rate, guarantee_next) = match m.rule.up {
        UpRule::FiftyFifty {
            rate,
            guarantee_next,
        } => (rate, guarantee_next),
        UpRule::Fixed { rate } => (rate, false),
        UpRule::None => (0.0, false),
    };
    let mut hit = 0.0;
    for s in 0..m.hard {
        let q = m.rule.curve.probability(s + 1);
        for g in [false, true] {
            for pp in 0..=m.path_max {
                let w = mass[idx(s, g, pp)];
                if w == 0.0 {
                    continue;
                }
                let path_full = m.path_max > 0 && pp >= m.path_max;
                let up_p = if g || path_full || m.next_top_is_up(i) {
                    1.0
                } else {
                    rate
                };
                hit += w * q * up_p;
                let lose = w * q * (1.0 - up_p);
                if lose > 0.0 {
                    let pp2 = if m.path_max > 0 {
                        (pp + 1).min(m.path_max)
                    } else {
                        0
                    };
                    next[idx(0, guarantee_next, pp2)] += lose;
                }
                if s + 1 < m.hard {
                    next[idx(s + 1, g, pp)] += w * (1.0 - q);
                }
            }
        }
    }
    hit
}

/// 曲线没有算到必得 UP 时，按规则估计最坏情况下多少抽内必得（没有保证时为空）。
fn up_guarantee_bound(m: &Model<'_>, since_top: u32, pity: &PityStatus, path0: u8) -> Option<u32> {
    let mut bounds = Vec::new();
    if let Some((n, true)) = m.up_within {
        bounds.push(n.saturating_sub(m.since_up).max(1));
    }
    if let Some(n) = m.spark {
        bounds.push(n.saturating_sub(m.banner_pulls).max(1));
    }
    let first = m.hard - since_top;
    let path_full = m.path_max > 0 && path0 >= m.path_max;
    match m.rule.up {
        _ if pity.guaranteed || path_full => bounds.push(first),
        UpRule::FiftyFifty {
            guarantee_next: true,
            ..
        } => bounds.push(if pity.guaranteed {
            first
        } else {
            first + m.hard
        }),
        _ if m.path_max > 0 => {
            let losses = u32::from(m.path_max - path0.min(m.path_max));
            bounds.push(first + losses * m.hard);
        }
        _ => {}
    }
    bounds.into_iter().min()
}

#[cfg(test)]
mod tests;
