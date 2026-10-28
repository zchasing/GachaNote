use std::collections::BTreeMap;

use super::*;
use crate::rules::{AchievementRule, PityCurve, PityScope, UpRule};
use crate::stats::{PityStatus, TopPull};

fn rule(up: UpRule, soft: Option<u32>) -> PoolRule {
    PoolRule {
        pool_type: "p".into(),
        since: None,
        rarity_top: 5,
        curve: PityCurve {
            base: 0.006,
            soft_start: soft,
            step: 0.06,
            hard: 90,
        },
        up,
        extras: vec![],
        pity_scope: PityScope::SharedByType,
        achievements: AchievementRule::default(),
    }
}

fn top(name: &str, pulls: u32, day: i64, up: UpStatus) -> TopPull {
    TopPull {
        index: 0,
        item_name: name.into(),
        pulled_at: day * DAY_MS,
        banner_id: None,
        pulls,
        up,
        estimated: false,
        was_guaranteed: false,
    }
}

fn stats(tops: Vec<TopPull>) -> PoolStats {
    PoolStats {
        pool_type: "p".into(),
        total_pulls: 0,
        free_count: 0,
        rarity_counts: BTreeMap::new(),
        tops,
        avg_pulls_per_top: None,
        up_count: 0,
        avg_pulls_per_up: None,
        loss: None,
        unknown_count: 0,
        estimated_count: 0,
        pity: PityStatus::default(),
    }
}

const HALF: UpRule = UpRule::FiftyFifty {
    rate: 0.5,
    guarantee_next: true,
};

#[test]
fn achievements_follow_thresholds() {
    let s = stats(vec![
        top("温迪", 31, 0, UpStatus::Up),
        top("刻晴", 78, 10, UpStatus::NotUp),
        top("可莉", 60, 70, UpStatus::Up),
        top("胡桃", 50, 80, UpStatus::Unknown),
        top("雷电", 40, 90, UpStatus::Up),
        top("夜兰", 40, 95, UpStatus::Up),
    ]);
    let a = achievements(&rule(HALF, Some(74)), &s);
    assert_eq!(a.len(), 4);
    // 90 × 0.35 = 31.5 → 31
    assert_eq!(
        (a[0].kind, a[0].done, a[0].value, a[0].threshold),
        (AchievementKind::Lucky, true, 31, 31)
    );
    assert_eq!(a[0].item_name.as_deref(), Some("温迪"));
    // 软保底 74 起第 5 抽 = 78
    assert_eq!(
        (a[1].kind, a[1].done, a[1].value, a[1].threshold),
        (AchievementKind::Unlucky, true, 78, 78)
    );
    // 可莉、（胡桃未知不打断）、雷电、夜兰
    assert_eq!(
        (a[2].kind, a[2].done, a[2].value),
        (AchievementKind::Streak, true, 3)
    );
    assert_eq!(
        (a[3].kind, a[3].done, a[3].value, a[3].threshold),
        (AchievementKind::LongWait, true, 60, 45)
    );
}

#[test]
fn achievements_not_reached_and_standard_pool() {
    let s = stats(vec![top("刻晴", 70, 0, UpStatus::NotUp)]);
    let a = achievements(&rule(UpRule::None, None), &s);
    // 常驻池没有不歪连胜；没有软保底时非酋线为硬保底
    assert_eq!(a.len(), 3);
    assert!(!a[0].done && !a[1].done);
    assert_eq!(a[1].threshold, 90);
    // 只有一只最高星时不算漫长等待
    assert!(!a[2].done);
    assert_eq!(a[2].value, 0);
    assert!(achievements(&rule(HALF, None), &stats(vec![])).is_empty());
}

fn rec(ms: i64, is_pull: bool) -> NewRecord {
    NewRecord {
        pool_type: "p".into(),
        banner_id: None,
        ext_id: None,
        item_id: None,
        item_name: "x".into(),
        item_type: None,
        rarity: 3,
        pulled_at: ms,
        seq_in_sec: 0,
        is_pull,
        raw_json: "{}".into(),
    }
}

#[test]
fn monthly_buckets_fill_gaps_and_use_timezone() {
    // 2026-01-31 20:00 UTC = 2026-02-01 04:00 UTC+8
    let jan31 = 1_769_889_600_000;
    let records = [
        rec(jan31, true),
        rec(jan31, false),
        rec(jan31 + 70 * DAY_MS, true),
    ];
    let refs: Vec<&NewRecord> = records.iter().collect();
    let mut t = top("温迪", 10, 0, UpStatus::Up);
    t.pulled_at = jan31;
    let s = stats(vec![t, top("刻晴", 10, 0, UpStatus::NotUp)]);
    let m = monthly(&refs[..2], &[&s], 480);
    assert_eq!(m.first().unwrap().month, "1970-01");
    let feb = m.iter().find(|b| b.month == "2026-02").unwrap();
    assert_eq!((feb.pulls, feb.tops, feb.ups), (1, 1, 1));
    let m = monthly(&refs, &[], 0);
    let months: Vec<&str> = m.iter().map(|b| b.month.as_str()).collect();
    assert_eq!(months, ["2026-01", "2026-02", "2026-03", "2026-04"]);
    assert_eq!(m[1].pulls, 0);
    assert_eq!(m[3].pulls, 1);
    assert!(monthly(&[], &[], 480).is_empty());
}

#[test]
fn month_of_known_dates() {
    assert_eq!(month_of(0), (1970, 1));
    assert_eq!(month_of(951_782_400_000), (2000, 2)); // 2000-02-29
    assert_eq!(month_of(1_798_675_200_000), (2026, 12)); // 2026-12-31
    assert_eq!(month_of(1_798_761_600_000), (2027, 1));
    assert_eq!(month_of(-DAY_MS), (1969, 12));
}
