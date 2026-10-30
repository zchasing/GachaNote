use super::*;
use crate::meta::Banner;
use crate::plugin::testing::TestGame;
use crate::rules::{Mechanic, PityCurve};
use proptest::prelude::*;

const T0: i64 = 1_700_000_000_000;

fn rec(pool: &str, name: &str, rarity: u8, i: i64) -> NewRecord {
    NewRecord {
        pool_type: pool.into(),
        banner_id: None,
        ext_id: Some(i.to_string()),
        item_id: None,
        item_name: name.into(),
        item_type: None,
        rarity,
        pulled_at: T0 + i * 1000,
        seq_in_sec: 0,
        is_pull: true,
        raw_json: "{}".into(),
    }
}

/// `n` 抽，最后一抽为 `top`，其余为三星。
fn segment(out: &mut Vec<NewRecord>, pool: &str, n: usize, top: Option<(&str, u8)>) {
    for k in 0..n {
        let i = out.len() as i64;
        let r = match top {
            Some((name, rarity)) if k == n - 1 => rec(pool, name, rarity, i),
            _ => rec(pool, "冷刃", 3, i),
        };
        out.push(r);
    }
}

fn meta() -> GameMeta {
    GameMeta {
        game: Some("genshin".into()),
        banners: vec![Banner {
            id: "b1".into(),
            pool_type: "character".into(),
            name: "b1".into(),
            start: T0,
            end: T0 + 1_000_000_000,
            up_top: vec!["温迪".into(), "可莉".into()],
            up_second: vec![],
        }],
        standard: vec!["刻晴".into(), "迪卢克".into()],
    }
}

fn run(pool: &str, rules: &[PoolRule], recs: &[NewRecord], meta: &GameMeta) -> PoolStats {
    let refs: Vec<&NewRecord> = recs.iter().collect();
    pool_stats(&pool.into(), rules, &refs, &|r| meta.classify(r)).unwrap()
}

fn curve(hard: u32) -> PityCurve {
    PityCurve {
        base: 0.006,
        soft_start: Some(74),
        step: 0.06,
        hard,
    }
}

fn rule(pool: &str, up: UpRule, extras: Vec<Mechanic>, scope: PityScope) -> PoolRule {
    PoolRule {
        pool_type: pool.into(),
        since: None,
        rarity_top: 5,
        curve: curve(90),
        up,
        extras,
        pity_scope: scope,
        achievements: Default::default(),
    }
}

#[test]
fn hand_computed_character_pool() {
    let mut r = Vec::new();
    segment(&mut r, "character", 10, Some(("刻晴", 5))); // 歪
    segment(&mut r, "character", 5, Some(("温迪", 5))); // 大保底
    segment(&mut r, "character", 20, Some(("可莉", 5))); // 小保底没歪
    segment(&mut r, "character", 7, None);
    let rules = TestGame::genshin().rules;
    let s = run("character", &rules, &r, &meta());

    assert_eq!(s.total_pulls, 42);
    assert_eq!(s.rarity_counts.get(&5), Some(&3));
    assert_eq!(s.rarity_counts.get(&3), Some(&39));
    let pulls: Vec<_> = s.tops.iter().map(|t| t.pulls).collect();
    assert_eq!(pulls, [10, 5, 20]);
    let ups: Vec<_> = s.tops.iter().map(|t| t.up).collect();
    assert_eq!(ups, [UpStatus::NotUp, UpStatus::Up, UpStatus::Up]);
    let guaranteed: Vec<_> = s.tops.iter().map(|t| t.was_guaranteed).collect();
    assert_eq!(guaranteed, [false, true, false]);
    assert_eq!(s.up_count, 2);
    // 平均出 UP：(10 + 5 + 20) / 2，尾部 7 抽不计
    assert_eq!(s.avg_pulls_per_up, Some(17.5));
    assert!((s.avg_pulls_per_top.unwrap() - 35.0 / 3.0).abs() < 1e-9);
    // 歪率：大保底那只不计入分母
    assert_eq!(s.loss, Some(LossRate { lost: 1, total: 2 }));
    assert_eq!(s.loss.unwrap().rate(), Some(0.5));
    assert_eq!(s.pity.since_top, 7);
    assert_eq!(s.pity.remaining_to_hard, 83);
    assert!(!s.pity.guaranteed);
    assert!(!s.pity.guarantee_uncertain);
    assert_eq!((s.pity.path_points, s.pity.since_up), (None, None));
    assert_eq!((s.unknown_count, s.estimated_count), (0, 0));
}

#[test]
fn ends_in_guarantee_after_loss() {
    let mut r = Vec::new();
    segment(&mut r, "character", 80, Some(("迪卢克", 5)));
    segment(&mut r, "character", 3, None);
    let s = run("character", &TestGame::genshin().rules, &r, &meta());
    assert!(s.pity.guaranteed);
    assert_eq!(s.pity.since_top, 3);
    assert_eq!(s.avg_pulls_per_up, None);
    assert_eq!(s.loss, Some(LossRate { lost: 1, total: 1 }));
}

#[test]
fn unsorted_input_is_sorted_by_time() {
    let mut r = Vec::new();
    segment(&mut r, "character", 10, Some(("刻晴", 5)));
    segment(&mut r, "character", 5, Some(("温迪", 5)));
    r.reverse();
    let s = run("character", &TestGame::genshin().rules, &r, &meta());
    let names: Vec<_> = s.tops.iter().map(|t| t.item_name.as_str()).collect();
    assert_eq!(names, ["刻晴", "温迪"]);
    assert_eq!(s.tops[0].index, 5);
}

#[test]
fn unknown_and_estimated_up() {
    let mut r = Vec::new();
    segment(&mut r, "character", 10, Some(("温迪", 5)));
    // 不在任何卡池时间内，按常驻名单估算
    let mut late = Vec::new();
    segment(&mut late, "character", 4, Some(("迪卢克", 5)));
    for x in &mut late {
        x.pulled_at += 2_000_000_000;
    }
    r.extend(late);
    let s = run("character", &TestGame::genshin().rules, &r, &meta());
    assert_eq!(s.estimated_count, 1);
    assert!(s.tops[1].estimated);
    assert!(s.pity.guaranteed);
    assert!(s.pity.guarantee_uncertain);

    // 没有元数据：UP 未知，不计入歪率，不推进大保底
    let s = run(
        "character",
        &TestGame::genshin().rules,
        &r,
        &GameMeta::default(),
    );
    assert_eq!(s.unknown_count, 2);
    assert_eq!(s.loss, Some(LossRate::default()));
    assert_eq!(s.loss.unwrap().rate(), None);
    assert_eq!(s.up_count, 0);
    assert!(!s.pity.guaranteed);
    assert!(s.pity.guarantee_uncertain);
}

#[test]
fn standard_pool_has_no_up_metrics() {
    let mut r = Vec::new();
    segment(&mut r, "standard", 30, Some(("刻晴", 5)));
    segment(&mut r, "standard", 2, None);
    let s = run("standard", &TestGame::genshin().rules, &r, &meta());
    assert_eq!(s.loss, None);
    assert_eq!(s.avg_pulls_per_up, None);
    assert_eq!(s.up_count, 0);
    assert_eq!(s.avg_pulls_per_top, Some(30.0));
    assert_eq!(s.tops[0].up, UpStatus::NotUp);
    assert_eq!(s.pity.since_top, 2);
}

#[test]
fn missing_rule_returns_none() {
    assert!(
        pool_stats(&"nope".into(), &TestGame::genshin().rules, &[], &|r| meta()
            .classify(r))
        .is_none()
    );
}

#[test]
fn empty_pool() {
    let s = run("character", &TestGame::genshin().rules, &[], &meta());
    assert_eq!(s.total_pulls, 0);
    assert_eq!(s.pity.remaining_to_hard, 90);
    assert_eq!(s.avg_pulls_per_top, None);
}

fn weapon_meta() -> GameMeta {
    let b = |id: &str, start: i64, up: &[&str]| Banner {
        id: id.into(),
        pool_type: "weapon".into(),
        name: id.into(),
        start: T0 + start * 1000,
        end: T0 + (start + 100) * 1000,
        up_top: up.iter().map(|s| (*s).to_owned()).collect(),
        up_second: vec![],
    };
    GameMeta {
        game: None,
        banners: vec![
            b("w1", 0, &["若水", "薙草之稻光"]),
            b("w2", 100, &["终末嗟叹之诗"]),
        ],
        standard: vec![],
    }
}

#[test]
fn epitomized_path_estimate_and_reset_on_banner_change() {
    let rules = vec![rule(
        "weapon",
        UpRule::FiftyFifty {
            rate: 0.75,
            guarantee_next: true,
        },
        vec![Mechanic::EpitomizedPath { max: 1 }],
        PityScope::SharedByType,
    )];
    let mut r = Vec::new();
    segment(&mut r, "weapon", 10, Some(("天空之刃", 5))); // 歪：命定值 +1，大保底
    segment(&mut r, "weapon", 10, Some(("若水", 5))); // 大保底 + 命定值满，归零
    segment(&mut r, "weapon", 10, Some(("和璞鸢", 5))); // 歪：命定值 1
    segment(&mut r, "weapon", 5, None);
    let m = weapon_meta();
    let s = run("weapon", &rules, &r, &m);
    assert_eq!(s.pity.path_points, Some(1));
    assert!(s.pity.guaranteed);
    assert_eq!(s.loss, Some(LossRate { lost: 2, total: 2 }));
    assert_eq!(s.pity.banner_pulls, 35);

    // 换期：命定值清零，保底计数继承
    segment(&mut r, "weapon", 70, None);
    segment(&mut r, "weapon", 1, None);
    let s = run("weapon", &rules, &r, &m);
    assert_eq!(s.pity.path_points, Some(0));
    assert_eq!(s.pity.since_top, 76);
    assert!(s.pity.guaranteed);
    assert_eq!(s.pity.banner_pulls, 6);
}

#[test]
fn path_counts_as_guarantee_even_without_fifty_fifty_guarantee() {
    let rules = vec![rule(
        "weapon",
        UpRule::FiftyFifty {
            rate: 0.75,
            guarantee_next: false,
        },
        vec![Mechanic::EpitomizedPath { max: 1 }],
        PityScope::SharedByType,
    )];
    let mut r = Vec::new();
    segment(&mut r, "weapon", 10, Some(("天空之刃", 5)));
    segment(&mut r, "weapon", 10, Some(("若水", 5)));
    let s = run("weapon", &rules, &r, &weapon_meta());
    assert!(s.tops[1].was_guaranteed);
    assert_eq!(s.loss, Some(LossRate { lost: 1, total: 1 }));
    assert!(!s.pity.guaranteed);
    assert_eq!(s.pity.path_points, Some(0));

    // UP 未知时命定值保持不变
    let s = run("weapon", &rules, &r, &GameMeta::default());
    assert_eq!(s.pity.path_points, Some(0));
    assert_eq!(s.unknown_count, 2);
}

#[test]
fn per_banner_scope_resets_pity() {
    let rules = vec![PoolRule {
        rarity_top: 6,
        ..rule(
            "limited",
            UpRule::Fixed { rate: 0.7 },
            vec![Mechanic::Spark { n: 300 }],
            PityScope::PerBanner,
        )
    }];
    let mut r = Vec::new();
    for i in 0..30 {
        let mut x = rec("limited", "芬", 3, i);
        x.banner_id = Some(if i < 20 { "ak1" } else { "ak2" }.into());
        if i == 9 {
            x.item_name = "银灰".into();
            x.rarity = 6;
        }
        r.push(x);
    }
    let m = GameMeta {
        game: None,
        banners: vec![],
        standard: vec!["银灰".into()],
    };
    let s = run("limited", &rules, &r, &m);
    assert_eq!(s.pity.since_top, 10);
    assert_eq!(s.pity.banner_pulls, 10);
    assert!(!s.pity.guaranteed);
    // Fixed：没有大小保底，歪率 = 非 UP 六星占比
    assert_eq!(s.loss, Some(LossRate { lost: 1, total: 1 }));
    assert_eq!(s.tops[0].banner_id.as_deref(), Some("ak1"));
}

#[test]
fn up_within_n_and_free_pulls() {
    let rules = vec![PoolRule {
        rarity_top: 6,
        curve: PityCurve {
            base: 0.008,
            soft_start: Some(65),
            step: 0.05,
            hard: 80,
        },
        ..rule(
            "special",
            UpRule::FiftyFifty {
                rate: 0.5,
                guarantee_next: false,
            },
            vec![
                Mechanic::UpWithinN {
                    n: 120,
                    per_banner: true,
                },
                Mechanic::FreePulls,
            ],
            PityScope::SharedByType,
        )
    }];
    let mut r = Vec::new();
    for i in 0..40 {
        let mut x = rec("special", "物资", 4, i);
        x.banner_id = Some(if i < 25 { "e1" } else { "e2" }.into());
        if i < 10 {
            x.is_pull = false; // 免费十连
        }
        if i == 5 {
            x.rarity = 6;
            x.item_name = "莱万汀".into();
        }
        if i == 20 {
            x.rarity = 6;
            x.item_name = "骏卫".into();
        }
        r.push(x);
    }
    let m = GameMeta {
        game: None,
        banners: vec![],
        standard: vec!["骏卫".into()],
    };
    let s = run("special", &rules, &r, &m);
    assert_eq!(s.free_count, 10);
    assert_eq!(s.total_pulls, 30);
    assert_eq!(s.rarity_counts.get(&6), Some(&1));
    // 免费抽出的六星不计数，也不进入保底序列
    assert_eq!(s.tops.len(), 1);
    assert_eq!(s.tops[0].pulls, 11);
    // 没有大保底：歪后不进入大保底
    assert!(!s.pity.guaranteed);
    assert_eq!(s.pity.since_top, 19);
    // 120 抽进度换期清零
    assert_eq!(s.pity.since_up, Some(15));
}

#[test]
fn rule_segments_by_date() {
    let mut early = rule(
        "weapon",
        UpRule::FiftyFifty {
            rate: 0.75,
            guarantee_next: true,
        },
        vec![Mechanic::EpitomizedPath { max: 2 }],
        PityScope::SharedByType,
    );
    early.curve = curve(80);
    let late = PoolRule {
        since: Some(T0 + 15_000),
        extras: vec![Mechanic::EpitomizedPath { max: 1 }],
        ..early.clone()
    };
    let rules = vec![early, late];
    let mut r = Vec::new();
    segment(&mut r, "weapon", 10, Some(("天空之刃", 5))); // 旧规则：命定值 1/2，大保底
    segment(&mut r, "weapon", 10, Some(("和璞鸢", 5))); // 新规则上限 1：命定值已满，这只算定轨必出
    let s = run("weapon", &rules, &r, &weapon_meta());
    assert_eq!(s.pity.remaining_to_hard, 80);
    assert!(s.tops[1].was_guaranteed);
    assert_eq!(s.pity.path_points, Some(0));
    assert_eq!(s.loss, Some(LossRate { lost: 1, total: 1 }));
}

#[test]
fn account_stats_groups_pools() {
    let plugin = TestGame::genshin();
    let mut data = Vec::new();
    segment(&mut data, "character", 10, Some(("温迪", 5)));
    segment(&mut data, "standard", 5, Some(("刻晴", 5)));
    data.push(rec("unknown_pool", "x", 3, 99));
    let records: Vec<GachaRecord> = data
        .into_iter()
        .enumerate()
        .map(|(i, d)| GachaRecord {
            id: i.to_string().into(),
            account_id: "a".into(),
            data: d,
            batch_id: None,
        })
        .collect();
    let s = account_stats(&plugin, &meta(), &records);
    assert_eq!(s.pools.len(), 2);
    assert_eq!(s.pools[0].pool_type, "character");
    assert_eq!(s.pools[0].up_count, 1);
    assert_eq!(s.total_pulls, 15);
    assert_eq!(s.rarity_counts.get(&5), Some(&2));
    assert_eq!(s.unrated, 1);
}

#[test]
fn account_stats_skips_duplicate_rule_segments() {
    let mut plugin = TestGame::genshin();
    let mut seg = plugin.rules[0].clone();
    seg.since = Some(1);
    plugin.rules.push(seg);
    let s = account_stats(&plugin, &meta(), &[]);
    assert_eq!(s.pools.len(), 2);
}

#[test]
fn record_marks_count_top_and_second() {
    let plugin = TestGame::genshin();
    let mut data = Vec::new();
    segment(&mut data, "character", 3, Some(("冷刃4", 4)));
    segment(&mut data, "character", 4, Some(("刻晴", 5)));
    segment(&mut data, "character", 2, Some(("温迪", 5)));
    segment(&mut data, "standard", 6, Some(("迪卢克", 5)));
    data.push(rec("unknown_pool", "x", 5, 99));
    let mut free = rec("character", "免费", 4, 100);
    free.is_pull = false;
    data.push(free);
    let records: Vec<GachaRecord> = data
        .into_iter()
        .enumerate()
        .map(|(i, d)| GachaRecord {
            id: i.to_string().into(),
            account_id: "a".into(),
            data: d,
            batch_id: None,
        })
        .collect();
    let marks = record_marks(&plugin, &meta(), &records);
    assert_eq!(marks.len(), records.len());
    // 四星：第 3 抽
    assert_eq!(marks[2].pulls, Some(3));
    assert_eq!(marks[2].up, None);
    // 五星：距上一只五星 7 抽，歪
    assert_eq!(marks[6].pulls, Some(7));
    assert_eq!(marks[6].up, Some(UpStatus::NotUp));
    assert!(!marks[6].estimated);
    // 大保底出 UP
    assert_eq!(marks[8].pulls, Some(2));
    assert_eq!(marks[8].up, Some(UpStatus::Up));
    // 三星不标注
    assert_eq!(marks[0], RecordMark::default());
    // 常驻池只有抽数
    assert_eq!(marks[14].pulls, Some(6));
    assert_eq!(marks[14].up, None);
    // 没有规则的卡池、免费抽不标注
    assert_eq!(marks[15], RecordMark::default());
    assert_eq!(marks[16], RecordMark::default());
}

proptest! {
    /// 不变量：保底计数不超过硬保底；各段抽数之和等于总抽数；歪率在 [0, 1]；
    /// 平均出 UP × UP 数 ≤ 总抽数。
    #[test]
    fn invariants(seq in proptest::collection::vec((0u8..100, any::<bool>(), any::<bool>()), 0..400)) {
        let hard = 90u32;
        let mut r = Vec::new();
        let mut since = 0u32;
        for (i, (roll, up, free)) in seq.iter().enumerate() {
            let free = *free && i % 7 == 0;
            let top = !free && (since + 1 >= hard || *roll < 2);
            let name = match (top, *up) {
                (true, true) => "温迪",
                (true, false) => "刻晴",
                _ => "冷刃",
            };
            let mut x = rec("character", name, if top { 5 } else { 3 }, i as i64);
            x.is_pull = !free;
            if !free {
                since = if top { 0 } else { since + 1 };
            }
            r.push(x);
        }
        let s = run("character", &TestGame::genshin().rules, &r, &meta());
        prop_assert!(s.pity.since_top < hard);
        prop_assert!(s.tops.iter().all(|t| t.pulls >= 1 && t.pulls <= hard));
        let sum: u32 = s.tops.iter().map(|t| t.pulls).sum();
        prop_assert_eq!(sum + s.pity.since_top, s.total_pulls);
        let loss = s.loss.unwrap();
        prop_assert!(loss.lost <= loss.total);
        if let Some(rate) = loss.rate() {
            prop_assert!((0.0..=1.0).contains(&rate));
        }
        if let Some(avg) = s.avg_pulls_per_up {
            prop_assert!(avg * f64::from(s.up_count) <= f64::from(s.total_pulls) + 1e-9);
        }
        // 大保底只会出现在歪之后
        for w in s.tops.windows(2) {
            prop_assert_eq!(w[1].was_guaranteed, w[0].up == UpStatus::NotUp && !w[0].was_guaranteed);
        }
    }
}
