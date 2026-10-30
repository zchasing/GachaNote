//! 欧非评价（架构文档 6.4，F-17）。
//!
//! 由单抽概率曲线得到「出一只最高星所需抽数」的理论分布 K，再按 UP 规则得到
//! 「出一只 UP 所需抽数」的分布 X：
//!
//! - 大小保底：X = rate · K + (1 − rate) · (K ∗ K)（歪了之后下一只必为 UP）；
//! - 固定概率（方舟、终末地）：X = Σ rate (1 − rate)^(j−1) · K^∗j（几何个最高星之和）。
//!
//! 用户的平均抽数是 n 个样本的均值，它在「n 个独立样本均值」的分布下的位置即为百分位：
//! 「比 x% 的人运气好」= P(别人的 n 样本均值 > 你的均值)。n 小时精确卷积，
//! n 大时用正态近似。井、N 抽必出 UP 只在少数情况下截断尾部，理论分布里不计入。

use serde::{Deserialize, Serialize};

use super::PoolStats;
use crate::rules::{PityCurve, PoolRule, UpRule};

/// 少于这么多只最高星时不评级（FR-ANA-04）。
pub const MIN_TOPS: u32 = 5;
/// 样本数不超过它时精确卷积，否则用正态近似。
const EXACT_MAX: u32 = 12;
/// 概率小于它的尾部截掉。
const TAIL: f64 = 1e-10;

/// 评价口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LuckMetric {
    /// 平均出 UP 抽数（有 UP 的卡池）。
    PerUp,
    /// 平均出最高星抽数（常驻池，或还没出过 UP）。
    PerTop,
}

/// 欧非评价。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Luck {
    /// 口径。
    pub metric: LuckMetric,
    /// 样本数（UP 数或最高星数）。
    pub samples: u32,
    /// 你的平均抽数。
    pub user_avg: f64,
    /// 理论平均抽数。
    pub expected_avg: f64,
    /// 比多少比例的人运气好（0–1）；最高星少于 [`MIN_TOPS`] 只时为空，只显示数值。
    pub percentile: Option<f64>,
}

/// 一个卡池的欧非评价；还没有出过最高星时为空。
pub fn luck(rule: &PoolRule, stats: &PoolStats) -> Option<Luck> {
    let tops = u32::try_from(stats.tops.len()).unwrap_or(u32::MAX);
    if tops == 0 {
        return None;
    }
    let k = top_pmf(&rule.curve);
    let (metric, samples, total, x) = match (rule.up, stats.avg_pulls_per_up) {
        (UpRule::None, _) | (_, None) => {
            let total: u64 = stats.tops.iter().map(|t| u64::from(t.pulls)).sum();
            (LuckMetric::PerTop, tops, total as f64, k)
        }
        (up, Some(avg)) => {
            let n = stats.up_count;
            (
                LuckMetric::PerUp,
                n,
                (avg * f64::from(n)).round(),
                up_pmf(up, &k),
            )
        }
    };
    let expected_avg = mean(&x);
    let percentile = (tops >= MIN_TOPS && samples > 0).then(|| better_than(&x, samples, total));
    Some(Luck {
        metric,
        samples,
        user_avg: total / f64::from(samples.max(1)),
        expected_avg,
        percentile,
    })
}

/// 出一只最高星所需抽数的分布：`pmf[k]` = P(第 k 抽才出)，`pmf[0] = 0`。
pub fn top_pmf(curve: &PityCurve) -> Vec<f64> {
    let hard = curve.hard.max(1);
    let mut pmf = vec![0.0; hard as usize + 1];
    let mut survive = 1.0;
    for k in 1..=hard {
        let p = curve.probability(k);
        pmf[k as usize] = survive * p;
        survive *= 1.0 - p;
    }
    pmf
}

/// 出一只 UP 所需抽数的分布。
fn up_pmf(up: UpRule, k: &[f64]) -> Vec<f64> {
    match up {
        UpRule::None => k.to_vec(),
        UpRule::FiftyFifty {
            rate,
            guarantee_next: true,
        } => {
            let two = convolve(k, k);
            let mut out = vec![0.0; two.len()];
            for (i, v) in out.iter_mut().enumerate() {
                *v = (1.0 - rate) * two[i] + rate * k.get(i).copied().unwrap_or(0.0);
            }
            out
        }
        UpRule::FiftyFifty { rate, .. } | UpRule::Fixed { rate } => {
            // 几何个最高星：逐项卷积，剩余权重可忽略时停止
            let mut out = k.to_vec();
            out.iter_mut().for_each(|v| *v *= rate);
            let mut power = k.to_vec();
            let mut weight = rate;
            while weight > TAIL && rate < 1.0 {
                weight *= 1.0 - rate;
                power = trim(convolve(&power, k));
                if out.len() < power.len() {
                    out.resize(power.len(), 0.0);
                }
                for (o, p) in out.iter_mut().zip(&power) {
                    *o += weight * p;
                }
            }
            trim(out)
        }
    }
}

fn convolve(a: &[f64], b: &[f64]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0.0; a.len() + b.len() - 1];
    for (i, &x) in a.iter().enumerate() {
        if x == 0.0 {
            continue;
        }
        for (j, &y) in b.iter().enumerate() {
            out[i + j] += x * y;
        }
    }
    out
}

/// 去掉末尾可忽略的尾部。
fn trim(mut v: Vec<f64>) -> Vec<f64> {
    let mut tail = 0.0;
    while let Some(&last) = v.last() {
        if tail + last > TAIL || v.len() <= 1 {
            break;
        }
        tail += last;
        v.pop();
    }
    v
}

fn mean(pmf: &[f64]) -> f64 {
    let mass: f64 = pmf.iter().sum();
    let m: f64 = pmf.iter().enumerate().map(|(i, p)| i as f64 * p).sum();
    m / mass.max(f64::MIN_POSITIVE)
}

fn variance(pmf: &[f64]) -> f64 {
    let mu = mean(pmf);
    let mass: f64 = pmf.iter().sum();
    let v: f64 = pmf
        .iter()
        .enumerate()
        .map(|(i, p)| (i as f64 - mu).powi(2) * p)
        .sum();
    v / mass.max(f64::MIN_POSITIVE)
}

/// P(别人 n 个样本的总抽数 > `total`)，相等时算一半。
fn better_than(x: &[f64], n: u32, total: f64) -> f64 {
    if n <= EXACT_MAX {
        let mut sum = vec![1.0];
        for _ in 0..n {
            sum = trim(convolve(&sum, x));
        }
        let mass: f64 = sum.iter().sum();
        let mut worse = 0.0;
        let mut tie = 0.0;
        for (i, p) in sum.iter().enumerate() {
            let i = i as f64;
            if i > total + 1e-9 {
                worse += p;
            } else if (i - total).abs() <= 1e-9 {
                tie += p;
            }
        }
        ((worse + tie / 2.0) / mass).clamp(0.0, 1.0)
    } else {
        let n = f64::from(n);
        let mu = mean(x) * n;
        let sd = (variance(x) * n).sqrt().max(1e-9);
        // 连续性修正：总抽数是整数
        1.0 - normal_cdf((total + 0.5 - mu) / sd)
    }
}

/// 标准正态分布函数（Abramowitz–Stegun 7.1.26，误差 < 1.5e-7）。
fn normal_cdf(z: f64) -> f64 {
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-x * x).exp();
    if z >= 0.0 {
        0.5 * (1.0 + erf)
    } else {
        0.5 * (1.0 - erf)
    }
}

#[cfg(test)]
mod tests;
