use super::*;
use crate::rules::{AchievementRule, Mechanic, PityCurve, PityScope};

fn curve(base: f64, soft: Option<u32>, step: f64, hard: u32) -> PityCurve {
    PityCurve {
        base,
        soft_start: soft,
        step,
        hard,
    }
}

fn rule(curve: PityCurve, up: UpRule, extras: Vec<Mechanic>) -> PoolRule {
    PoolRule {
        pool_type: "p".into(),
        since: None,
        rarity_top: 5,
        curve,
        up,
        extras,
        pity_scope: PityScope::SharedByType,
        achievements: AchievementRule::default(),
    }
}

fn genshin_char() -> PoolRule {
    rule(
        curve(0.006, Some(74), 0.06, 90),
        UpRule::FiftyFifty {
            rate: 0.5,
            guarantee_next: true,
        },
        vec![],
    )
}

fn pity(since_top: u32) -> PityStatus {
    PityStatus {
        since_top,
        ..PityStatus::default()
    }
}

/// 不依赖 DP 的逐抽模拟（SplitMix64，固定种子，结果可复现）。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// 模拟 `trials` 次，返回「第 k 抽时已出最高星 / 已出 UP」的经验累计频率（k = 0..=pulls）。
fn simulate(r: &PoolRule, start: &PityStatus, pulls: u32, trials: u32) -> (Vec<f64>, Vec<f64>) {
    let mut rng = Rng(42);
    let mut top_at = vec![0u32; pulls as usize + 2];
    let mut up_at = vec![0u32; pulls as usize + 2];
    let (rate, guarantee_next) = match r.up {
        UpRule::FiftyFifty {
            rate,
            guarantee_next,
        } => (rate, guarantee_next),
        UpRule::Fixed { rate } => (rate, false),
        UpRule::None => (0.0, false),
    };
    let path_max = r.path_max().unwrap_or(0);
    for _ in 0..trials {
        let mut since_top = start.since_top;
        let mut guaranteed = start.guaranteed;
        let mut path = start.path_points.unwrap_or(0);
        let mut since_up = start.since_up.unwrap_or(0);
        let active = match r.up_within() {
            Some((_, true)) => since_up >= start.banner_pulls,
            Some((_, false)) => true,
            None => false,
        };
        let mut first_top = None;
        let mut first_up = None;
        for i in 1..=pulls {
            since_top += 1;
            since_up += 1;
            let banner = start.banner_pulls + i;
            let mut got_up = false;
            match r.up_within() {
                Some((n, true)) if active && since_up >= n => {
                    first_top.get_or_insert(i);
                    got_up = true;
                }
                _ => {
                    if rng.next() < r.curve.probability(since_top) {
                        first_top.get_or_insert(i);
                        since_top = 0;
                        let forced = matches!(r.up_within(), Some((n, false)) if since_up > n);
                        let full = path_max > 0 && path >= path_max;
                        if r.up.has_up() {
                            if guaranteed || full || forced || rng.next() < rate {
                                got_up = true;
                            } else {
                                guaranteed = guarantee_next;
                                path = (path + 1).min(path_max);
                            }
                        }
                    }
                }
            }
            if r.up.has_up() && r.spark().is_some_and(|n| banner >= n) {
                got_up = true;
            }
            if got_up {
                first_up.get_or_insert(i);
            }
            // 井兑换 UP 不影响保底，继续抽到出最高星为止
            if first_top.is_some() && first_up.is_some() {
                break;
            }
        }
        if let Some(i) = first_top {
            top_at[i as usize] += 1;
        }
        if let Some(i) = first_up {
            up_at[i as usize] += 1;
        }
    }
    let cdf = |hist: &[u32]| {
        let mut acc = 0u32;
        hist.iter()
            .take(pulls as usize + 1)
            .map(|&h| {
                acc += h;
                f64::from(acc) / f64::from(trials)
            })
            .collect::<Vec<_>>()
    };
    (cdf(&top_at), cdf(&up_at))
}

/// DP 与 20 万次模拟逐点比较，误差 < 0.5 个百分点（T-54 验收标准）。
fn assert_matches_simulation(r: &PoolRule, start: &PityStatus, pulls: u32) {
    let p = predict(r, start, pulls);
    let (top, up) = simulate(r, start, pulls, 200_000);
    let mut worst: f64 = 0.0;
    for k in 0..=pulls {
        let c = p.at(k);
        worst = worst.max((c.top - top[k as usize]).abs());
        if let Some(u) = c.up {
            worst = worst.max((u - up[k as usize]).abs());
        }
    }
    assert!(worst < 0.005, "最大误差 {worst}");
}

#[test]
fn matches_monte_carlo_fifty_fifty() {
    assert_matches_simulation(&genshin_char(), &pity(30), 200);
    let guaranteed = PityStatus {
        since_top: 75,
        guaranteed: true,
        ..PityStatus::default()
    };
    assert_matches_simulation(&genshin_char(), &guaranteed, 120);
}

#[test]
fn matches_monte_carlo_epitomized_path() {
    let weapon = rule(
        curve(0.007, Some(63), 0.07, 80),
        UpRule::FiftyFifty {
            rate: 0.75,
            guarantee_next: true,
        },
        vec![Mechanic::EpitomizedPath { max: 1 }],
    );
    let start = PityStatus {
        since_top: 10,
        path_points: Some(0),
        ..PityStatus::default()
    };
    assert_matches_simulation(&weapon, &start, 170);
}

#[test]
fn matches_monte_carlo_up_within_n() {
    let ak = curve(0.02, Some(51), 0.02, 99);
    // 方舟登场限定：满 150 抽后下一只六星必为 UP
    let single = rule(
        ak,
        UpRule::Fixed { rate: 0.5 },
        vec![Mechanic::UpWithinN {
            n: 150,
            per_banner: false,
        }],
    );
    let start = PityStatus {
        since_top: 20,
        since_up: Some(140),
        banner_pulls: 140,
        ..PityStatus::default()
    };
    assert_matches_simulation(&single, &start, 200);
    // 终末地：当期第 120 抽必出 UP
    let ef = rule(
        curve(0.008, Some(65), 0.05, 80),
        UpRule::Fixed { rate: 0.5 },
        vec![Mechanic::UpWithinN {
            n: 120,
            per_banner: true,
        }],
    );
    let start = PityStatus {
        since_top: 30,
        since_up: Some(30),
        banner_pulls: 30,
        ..PityStatus::default()
    };
    assert_matches_simulation(&ef, &start, 100);
    let p = predict(&ef, &start, 10);
    assert_eq!(p.at(90).up, Some(1.0));
    assert_eq!(p.at(90).top, 1.0);
    assert_eq!(p.up_guaranteed_within, Some(90));
}

#[test]
fn matches_monte_carlo_spark() {
    let limited = rule(
        curve(0.02, Some(51), 0.02, 99),
        UpRule::Fixed { rate: 0.7 },
        vec![Mechanic::Spark { n: 300 }],
    );
    let start = PityStatus {
        since_top: 5,
        banner_pulls: 250,
        ..PityStatus::default()
    };
    assert_matches_simulation(&limited, &start, 60);
    let p = predict(&limited, &start, 10);
    assert_eq!(p.at(50).up, Some(1.0));
    assert_eq!(p.up_guaranteed_within, Some(50));
}

#[test]
fn standard_pool_has_no_up_curve() {
    let std = rule(curve(0.006, Some(74), 0.06, 90), UpRule::None, vec![]);
    let p = predict(&std, &pity(89), 5);
    assert_eq!(p.at(1).top, 1.0);
    assert_eq!(p.at(1).up, None);
    assert_eq!(p.top_guaranteed_within, 1);
    assert_eq!(p.up_guaranteed_within, None);
    assert_matches_simulation(&std, &pity(0), 90);
}

#[test]
fn top_probability_is_product_formula() {
    let r = genshin_char();
    let p = predict(&r, &pity(70), 20);
    let mut survive = 1.0;
    for k in 1..=20u32 {
        survive *= 1.0 - r.curve.probability(70 + k);
        assert!((p.at(k).top - (1.0 - survive)).abs() < 1e-12, "k = {k}");
    }
    // 第 20 抽到达硬保底
    assert_eq!(p.at(20).top, 1.0);
    assert_eq!(p.top_guaranteed_within, 20);
    // 小保底：最坏 20 + 90 抽必得 UP
    assert_eq!(p.up_guaranteed_within, Some(110));
}

#[test]
fn curve_extends_until_settled_and_quantiles() {
    let p = predict(&genshin_char(), &pity(0), 10);
    // 请求 10 抽，但曲线延长到出 UP 基本确定
    assert!(p.points.len() > 100);
    assert!(p
        .points
        .iter()
        .enumerate()
        .all(|(i, c)| c.pulls as usize == i));
    let q50 = p.quantile(0.5, false).unwrap();
    let q90 = p.quantile(0.9, false).unwrap();
    assert!(q50 < q90 && q90 <= 90, "{q50} {q90}");
    let u50 = p.quantile(0.5, true).unwrap();
    assert!(u50 >= q50);
    assert_eq!(p.quantile(1.1, false), None);
    // 超出曲线时取最后一点
    assert_eq!(p.at(5000), *p.points.last().unwrap());
    assert_eq!(p.at(0).up, Some(0.0));
    // 曲线单调不减
    assert!(p
        .points
        .windows(2)
        .all(|w| w[1].top >= w[0].top - 1e-12 && w[1].up >= w[0].up));
}

#[test]
fn guarantee_bounds() {
    let r = genshin_char();
    let g = PityStatus {
        since_top: 10,
        guaranteed: true,
        ..PityStatus::default()
    };
    assert_eq!(predict(&r, &g, 1).up_guaranteed_within, Some(80));
    // 命定值已满
    let weapon = rule(
        curve(0.007, Some(63), 0.07, 80),
        UpRule::Fixed { rate: 0.75 },
        vec![Mechanic::EpitomizedPath { max: 2 }],
    );
    let s = PityStatus {
        since_top: 0,
        path_points: Some(2),
        ..PityStatus::default()
    };
    let p = predict(&weapon, &s, 1);
    assert_eq!(p.up_guaranteed_within, Some(80));
    let s = PityStatus {
        path_points: Some(1),
        ..s
    };
    assert_eq!(predict(&weapon, &s, 1).up_guaranteed_within, Some(160));
    // 方舟标准寻访：没有任何保证
    let ak = rule(
        curve(0.02, Some(51), 0.02, 99),
        UpRule::Fixed { rate: 0.5 },
        vec![],
    );
    let p = predict(&ak, &pity(0), 10);
    assert_eq!(p.up_guaranteed_within, None);
    // 当期已出过 UP：每期一次的 120 抽保证不再生效
    let ef = rule(
        curve(0.008, Some(65), 0.05, 80),
        UpRule::Fixed { rate: 0.5 },
        vec![Mechanic::UpWithinN {
            n: 120,
            per_banner: true,
        }],
    );
    let spent = PityStatus {
        since_top: 0,
        since_up: Some(10),
        banner_pulls: 100,
        ..PityStatus::default()
    };
    assert_eq!(predict(&ef, &spent, 1).up_guaranteed_within, None);
    // 已垫抽数超过硬保底（规则变化）时按硬保底前一抽处理
    let over = predict(&r, &pity(200), 1);
    assert_eq!(over.at(1).top, 1.0);
}

#[test]
fn request_is_capped() {
    let p = predict(&genshin_char(), &pity(0), 5000);
    assert_eq!(p.points.len() as u32, MAX_PULLS + 1);
}
