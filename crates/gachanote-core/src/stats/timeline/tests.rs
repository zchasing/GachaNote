use super::*;
use crate::meta::UpStatus;

/// 2023-11-14 22:13:20 UTC，北京时间 11-15 06:13:20。
const T0: i64 = 1_700_000_000_000;

fn rec(name: &str, rarity: u8, at: i64) -> NewRecord {
    NewRecord {
        pool_type: "character".into(),
        banner_id: None,
        ext_id: None,
        item_id: None,
        item_name: name.into(),
        item_type: None,
        rarity,
        pulled_at: at,
        seq_in_sec: 0,
        is_pull: true,
        raw_json: "{}".into(),
    }
}

fn top(index: usize, name: &str, at: i64) -> TopPull {
    TopPull {
        index,
        item_name: name.into(),
        pulled_at: at,
        banner_id: None,
        pulls: 10,
        up: UpStatus::Up,
        estimated: false,
        was_guaranteed: false,
    }
}

fn banner(id: &str, start: i64, end: i64) -> Banner {
    Banner {
        id: id.into(),
        pool_type: "character".into(),
        name: id.into(),
        start,
        end,
        up_top: vec![],
        up_second: vec![],
    }
}

#[test]
fn daily_fills_gaps_and_splits_tiers() {
    let mut free = rec("赠送", 3, T0);
    free.is_pull = false;
    let recs = [
        rec("温迪", 5, T0),
        rec("香菱", 4, T0 + 1000),
        rec("冷刃", 3, T0 + 2000),
        free,
        rec("冷刃", 3, T0 + 2 * DAY_MS),
    ];
    let refs: Vec<&NewRecord> = recs.iter().collect();
    let d = daily(&refs, 5, 480, T0 + 3 * DAY_MS);
    // 按北京时间算，第一条在 11-15
    assert_eq!(d.start, "2023-11-15");
    assert_eq!(d.counts, [[1, 1, 1], [0, 0, 0], [0, 0, 1], [0, 0, 0]]);
    // UTC 时第一条在 11-14
    assert_eq!(daily(&refs, 5, 0, 0).start, "2023-11-14");
    assert_eq!(daily(&[], 5, 480, T0), Daily::default());
}

#[test]
fn banner_periods_include_skipped_ones() {
    let b1 = banner("b1", T0 - DAY_MS, T0 + 10 * DAY_MS);
    let b2 = banner("b2", T0 + 10 * DAY_MS, T0 + 20 * DAY_MS);
    let b3 = banner("b3", T0 + 20 * DAY_MS, T0 + 30 * DAY_MS);
    let future = banner("future", T0 + 99 * DAY_MS, T0 + 120 * DAY_MS);
    let old = banner("old", T0 - 30 * DAY_MS, T0 - 20 * DAY_MS);
    let recs = [
        rec("冷刃", 3, T0),
        rec("温迪", 5, T0 + DAY_MS),
        rec("冷刃", 3, T0 + 25 * DAY_MS),
    ];
    let refs: Vec<&NewRecord> = recs.iter().collect();
    let tops = [top(1, "温迪", T0 + DAY_MS)];
    let banners = [&old, &b1, &b2, &b3, &future];
    let ps = periods(&refs, &tops, &banners, T0 + 40 * DAY_MS);
    let ids: Vec<_> = ps.iter().map(|p| p.id.as_deref().unwrap()).collect();
    assert_eq!(ids, ["b3", "b2", "b1"]);
    assert_eq!(ps[0].pulls, 1);
    assert_eq!(ps[1].pulls, 0);
    assert_eq!(ps[1].first_record, None);
    assert_eq!(ps[2].pulls, 2);
    assert_eq!(ps[2].tops.len(), 1);
    assert_eq!(ps[2].tops[0].item_name, "温迪");
    assert_eq!((ps[2].start, ps[2].end), (b1.start, b1.end));
    assert!(ps.iter().all(|p| p.source == PeriodSource::Banner));
}

#[test]
fn record_banner_id_wins_over_time() {
    let b1 = banner("b1", T0, T0 + 10 * DAY_MS);
    let b2 = banner("b2", T0, T0 + 10 * DAY_MS);
    let mut r = rec("冷刃", 3, T0 + DAY_MS);
    r.banner_id = Some("b2".into());
    let refs = [&r];
    let ps = periods(&refs, &[], &[&b1, &b2], T0 + DAY_MS);
    let joined: Vec<_> = ps.iter().filter(|p| p.pulls > 0).collect();
    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].id.as_deref(), Some("b2"));
}

#[test]
fn pool_id_groups_without_metadata() {
    let mut a = rec("能天使", 6, T0);
    a.banner_id = Some("SINGLE_1".into());
    let mut b = rec("芬", 3, T0 + 2 * DAY_MS);
    b.banner_id = Some("SINGLE_1".into());
    let mut c = rec("芬", 3, T0 + 5 * DAY_MS);
    c.banner_id = Some("SINGLE_2".into());
    let refs = [&a, &b, &c];
    let ps = periods(&refs, &[top(0, "能天使", T0)], &[], T0);
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].id.as_deref(), Some("SINGLE_2"));
    assert_eq!(ps[1].source, PeriodSource::Pool);
    assert_eq!((ps[1].start, ps[1].end), (T0, T0 + 2 * DAY_MS));
    assert_eq!((ps[1].pulls, ps[1].tops.len()), (2, 1));
    assert_eq!(ps[1].first_record, Some(0));
}

#[test]
fn auto_segments_by_gap_and_length() {
    let mut recs = Vec::new();
    // 第一段：每天一抽，连续 30 天，超过 21 天后另起一段
    for d in 0..30 {
        recs.push(rec("冷刃", 3, T0 + d * DAY_MS));
    }
    // 隔 10 天再抽，另起一段
    recs.push(rec("冷刃", 3, T0 + 40 * DAY_MS));
    let refs: Vec<&NewRecord> = recs.iter().collect();
    let ps = periods(&refs, &[], &[], T0 + 40 * DAY_MS);
    assert!(ps
        .iter()
        .all(|p| p.source == PeriodSource::Auto && p.id.is_none()));
    let spans: Vec<_> = ps
        .iter()
        .map(|p| ((p.start - T0) / DAY_MS, (p.end - T0) / DAY_MS, p.pulls))
        .collect();
    assert_eq!(spans, [(40, 40, 1), (22, 29, 8), (0, 21, 22)]);
}
