use std::collections::BTreeMap;

use super::*;
use crate::meta::UpStatus;
use crate::rules::{AchievementRule, PityScope};
use crate::stats::{PityStatus, TopPull};

fn curve() -> PityCurve {
    PityCurve {
        base: 0.006,
        soft_start: Some(74),
        step: 0.06,
        hard: 90,
    }
}

fn rule(up: UpRule) -> PoolRule {
    PoolRule {
        pool_type: "p".into(),
        since: None,
        rarity_top: 5,
        curve: curve(),
        up,
        extras: vec![],
        pity_scope: PityScope::SharedByType,
        achievements: AchievementRule::default(),
    }
}

/// 每只最高星都花 `pulls` 抽、都是 UP 的卡池统计。
fn stats(pulls: &[u32], ups: u32) -> PoolStats {
    let tops: Vec<TopPull> = pulls
        .iter()
        .enumerate()
        .map(|(i, &p)| TopPull {
            index: i,
            item_name: "x".into(),
            pulled_at: i as i64,
            banner_id: None,
            pulls: p,
            up: UpStatus::Up,
            estimated: false,
            was_guaranteed: false,
        })
        .collect();
    let total: u32 = pulls.iter().sum();
    PoolStats {
        pool_type: "p".into(),
        total_pulls: total,
        free_count: 0,
        rarity_counts: BTreeMap::new(),
        avg_pulls_per_top: None,
        up_count: ups,
        avg_pulls_per_up: (ups > 0).then(|| f64::from(total) / f64::from(ups)),
        loss: None,
        unknown_count: 0,
        estimated_count: 0,
        pity: PityStatus::default(),
        tops,
    }
}

const HALF: UpRule = UpRule::FiftyFifty {
    rate: 0.5,
    guarantee_next: true,
};

#[test]
fn distributions_have_expected_means() {
    let k = top_pmf(&curve());
    assert!((k.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    assert!((mean(&k) - curve().expected_pulls()).abs() < 1e-9);
    let x = up_pmf(HALF, &k);
    assert!((x.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    assert!((mean(&x) - 1.5 * mean(&k)).abs() < 1e-6);
    let g = up_pmf(UpRule::Fixed { rate: 0.25 }, &k);
    assert!((mean(&g) - 4.0 * mean(&k)).abs() < 1e-3, "{}", mean(&g));
    assert_eq!(up_pmf(UpRule::None, &k), k);
    // 常驻曲线到硬保底为止
    assert_eq!(k.len(), 91);
    assert_eq!(k[0], 0.0);
}

#[test]
fn not_rated_below_five_tops() {
    let r = rule(HALF);
    assert_eq!(luck(&r, &stats(&[], 0)), None);
    let l = luck(&r, &stats(&[40, 50, 60, 70], 4)).unwrap();
    assert_eq!(l.percentile, None);
    assert_eq!(l.metric, LuckMetric::PerUp);
    assert_eq!(l.samples, 4);
    assert!((l.user_avg - 55.0).abs() < 1e-9);
    assert!((l.expected_avg - 1.5 * curve().expected_pulls()).abs() < 1e-6);
}

#[test]
fn percentile_orders_luck() {
    let r = rule(HALF);
    let lucky = luck(&r, &stats(&[20, 30, 25, 40, 35], 5)).unwrap();
    let unlucky = luck(&r, &stats(&[85, 88, 90, 80, 89], 3)).unwrap();
    let a = lucky.percentile.unwrap();
    let b = unlucky.percentile.unwrap();
    assert!(a > 0.95, "{a}");
    assert!(b < 0.1, "{b}");
    // 平均接近理论值时在中间
    let mid = (1.5 * curve().expected_pulls()).round() as u32;
    let avg = luck(&r, &stats(&[mid; 6], 6)).unwrap().percentile.unwrap();
    assert!(avg > 0.3 && avg < 0.7, "{avg}");
}

#[test]
fn standard_pool_uses_pulls_per_top() {
    let l = luck(&rule(UpRule::None), &stats(&[62, 62, 63, 62, 62], 0)).unwrap();
    assert_eq!(l.metric, LuckMetric::PerTop);
    assert_eq!(l.samples, 5);
    let p = l.percentile.unwrap();
    assert!(p > 0.3 && p < 0.7, "{p}");
    // 有 UP 规则但还没出过 UP，也按出金评价
    let l = luck(&rule(HALF), &stats(&[62, 62, 63, 62, 62], 0)).unwrap();
    assert_eq!(l.metric, LuckMetric::PerTop);
}

#[test]
fn exact_and_normal_agree_near_switch() {
    let x = top_pmf(&curve());
    let mu = mean(&x);
    let exact = better_than(&x, EXACT_MAX, (mu * 0.9 * f64::from(EXACT_MAX)).round());
    let approx = better_than(
        &x,
        EXACT_MAX + 1,
        (mu * 0.9 * f64::from(EXACT_MAX + 1)).round(),
    );
    assert!((exact - approx).abs() < 0.06, "{exact} {approx}");
    assert!(exact > 0.5 && approx > 0.5);
}

#[test]
fn exact_percentile_matches_enumeration() {
    // 两点分布：1 或 3 抽各一半；两个样本总和 2 / 4 / 6 的概率为 1/4、1/2、1/4
    let x = vec![0.0, 0.5, 0.0, 0.5];
    assert!((better_than(&x, 2, 4.0) - (0.25 + 0.25)).abs() < 1e-12);
    assert!((better_than(&x, 2, 2.0) - (0.75 + 0.125)).abs() < 1e-12);
    assert!((better_than(&x, 2, 7.0)).abs() < 1e-12);
}

#[test]
fn helpers() {
    assert!((normal_cdf(0.0) - 0.5).abs() < 1e-7);
    assert!((normal_cdf(1.96) - 0.975).abs() < 1e-3);
    assert!((normal_cdf(-1.96) - 0.025).abs() < 1e-3);
    assert!(convolve(&[], &[1.0]).is_empty());
    assert_eq!(trim(vec![0.5, 0.5, 1e-12, 1e-13]), vec![0.5, 0.5]);
    assert_eq!(trim(vec![1e-13]), vec![1e-13]);
    assert!(variance(&[0.0, 1.0]).abs() < 1e-12);
}
