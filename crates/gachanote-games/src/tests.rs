use std::path::PathBuf;

use gachanote_core::import::normalize;
use gachanote_core::meta::GameMeta;
use gachanote_core::model::{GachaRecord, NewRecord};
use gachanote_core::plugin::{FetchKind, Vendor};
use gachanote_core::rules::{rule_at, UpRule};
use gachanote_core::stats::account_stats;
use gachanote_core::FormatId;

use super::*;
use arknights::Arknights;
use endfield::Endfield;

fn registry() -> PluginRegistry {
    let mut reg = PluginRegistry::new();
    register_all(
        &mut reg,
        &default_client(),
        &Pacing::default(),
        &HgClient::new(default_client(), Pacing::default()),
    );
    reg
}

fn sample(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../gachanote-formats/tests/samples")
        .join(name)
}

#[test]
fn six_games_registered_with_consistent_info() {
    let reg = registry();
    let ids: Vec<_> = reg.games().iter().map(|g| g.id().to_string()).collect();
    assert_eq!(
        ids,
        [
            "genshin",
            "starrail",
            "zzz",
            "arknights",
            "endfield",
            "wuwa"
        ]
    );
    for g in reg.games() {
        let info = g.info();
        assert_eq!(info.id, g.id());
        assert!(info.theme_color.starts_with('#') && info.theme_color.len() == 7);
        assert!(!info.formats.is_empty());
        assert!(info.formats.contains(&FormatId::GnCsv));
        // 每个卡池类型都有规则，且最新规则的最高星与硬保底合理
        for p in &info.pool_types {
            let r = rule_at(g.rules(), p, None).unwrap();
            assert!((4..=6).contains(&r.rarity_top), "{p}");
            assert!((40..=99).contains(&r.curve.hard), "{p}");
            assert!(r.curve.base > 0.0 && r.curve.base < 0.05, "{p}");
            if let UpRule::FiftyFifty { rate, .. } | UpRule::Fixed { rate } = r.up {
                assert!(rate > 0.0 && rate <= 1.0);
            }
        }
        assert_eq!(info.fetch.is_some(), g.fetcher().is_some());
        if let Some(f) = g.fetcher() {
            // 鹰角两款用账号 token，其余用抽卡链接
            let token = info.vendor == Vendor::Hypergryph;
            assert_eq!(f.kind() == FetchKind::AccountToken, token);
            assert_eq!(info.fetch, Some(f.kind()));
        }
    }
    let vendors: Vec<_> = reg.games().iter().map(|g| g.info().vendor).collect();
    assert_eq!(vendors[0], Vendor::Hoyoverse);
    assert_eq!(vendors[3], Vendor::Hypergryph);
    assert_eq!(vendors[5], Vendor::Kuro);
    assert_eq!(
        reg.converter(FormatId::UigfV4).unwrap().format(),
        FormatId::UigfV4
    );
    assert_eq!(
        Genshin::default().id(),
        GameId::from(GameId::GENSHIN),
        "Default 构造"
    );
    let _ = (StarRail::default(), Zzz::default(), Wuwa::default());
    assert!(Arknights::default().fetcher().is_none());
    assert!(Endfield::default().fetcher().is_none());
}

use genshin::Genshin;
use starrail::StarRail;
use wuwa::Wuwa;
use zzz::Zzz;

#[test]
fn pool_type_mappings() {
    let m = |f: fn(&str) -> Option<PoolTypeId>, code: &str| f(code).map(|p| p.to_string());
    assert_eq!(m(genshin::pool_type, "400").as_deref(), Some("character"));
    assert_eq!(m(genshin::pool_type, "302").as_deref(), Some("weapon"));
    assert_eq!(m(genshin::pool_type, "500").as_deref(), Some("chronicled"));
    assert_eq!(m(genshin::pool_type, "100").as_deref(), Some("beginner"));
    assert_eq!(m(genshin::pool_type, "200").as_deref(), Some("standard"));
    assert_eq!(m(genshin::pool_type, "1"), None);
    assert_eq!(
        m(starrail::pool_type, "21").as_deref(),
        Some("collab_character")
    );
    assert_eq!(
        m(starrail::pool_type, "22").as_deref(),
        Some("collab_light_cone")
    );
    assert_eq!(m(starrail::pool_type, "3"), None);
    assert_eq!(m(zzz::pool_type, "2001").as_deref(), Some("character"));
    assert_eq!(m(zzz::pool_type, "3002").as_deref(), Some("w_engine"));
    assert_eq!(m(zzz::pool_type, "5").as_deref(), Some("bangboo"));
    assert_eq!(
        m(zzz::pool_type, "102").as_deref(),
        Some("exclusive_rescreening")
    );
    assert_eq!(
        m(zzz::pool_type, "103").as_deref(),
        Some("w_engine_reverberation")
    );
    assert_eq!(m(zzz::pool_type, "9001"), None);
    assert_eq!(m(zzz::pool_type, "x"), None);
    assert_eq!(
        m(arknights::pool_type, "LIMITED_37_0_1").as_deref(),
        Some("limited")
    );
    assert_eq!(
        m(arknights::pool_type, "NORM_1").as_deref(),
        Some("standard")
    );
    assert_eq!(
        m(arknights::pool_type, "DOUBLE_59_0_2").as_deref(),
        Some("standard")
    );
    assert_eq!(
        m(arknights::pool_type, "FESCLASSIC_1").as_deref(),
        Some("classic")
    );
    assert_eq!(
        m(arknights::pool_type, "SINGLE_1").as_deref(),
        Some("single")
    );
    assert_eq!(
        m(arknights::pool_type, "LINKAGE_1").as_deref(),
        Some("linkage")
    );
    assert_eq!(
        m(arknights::pool_type, "BOOT_0_1_1").as_deref(),
        Some("beginner")
    );
    assert_eq!(
        m(arknights::pool_type, "ATTAIN_1").as_deref(),
        Some("other")
    );
    assert_eq!(m(arknights::pool_type, ""), None);
    assert_eq!(
        m(endfield::pool_type, "char:special_1_0_1").as_deref(),
        Some("character")
    );
    assert_eq!(
        m(endfield::pool_type, "char:standard").as_deref(),
        Some("standard")
    );
    assert_eq!(
        m(endfield::pool_type, "char:beginner").as_deref(),
        Some("beginner")
    );
    assert_eq!(
        m(endfield::pool_type, "char:joint_1").as_deref(),
        Some("other")
    );
    assert_eq!(
        m(endfield::pool_type, "weapon:wpn_1").as_deref(),
        Some("weapon")
    );
    assert_eq!(m(endfield::pool_type, "pet:x"), None);
    assert_eq!(m(endfield::pool_type, "nocolon"), None);
    assert_eq!(
        m(wuwa::pool_type, "7").as_deref(),
        Some("giveback_selector")
    );
    assert_eq!(m(wuwa::pool_type, "8"), None);
}

#[test]
fn uid_rules() {
    let reg = registry();
    let ok = |g: &str, uid: &str| reg.game(&g.into()).unwrap().validate_uid(uid);
    assert!(ok("genshin", "100000001") && ok("genshin", "1800000001"));
    assert!(!ok("genshin", "10000001"));
    assert!(ok("starrail", "100000001"));
    assert!(ok("zzz", "10000001") && ok("zzz", "1300000001"));
    assert!(ok("arknights", "12345678") && !ok("arknights", "abc"));
    assert!(ok("endfield", "10000005"));
    assert!(ok("wuwa", "100000001") && !ok("wuwa", "12345"));
}

#[test]
fn detects_pasted_urls() {
    assert_eq!(
        detect_url(
            "https://webstatic.mihoyo.com/hkrpg/event/index.html?authkey=1&game_biz=hkrpg_cn"
        ),
        Some(GameId::STARRAIL.into())
    );
    assert_eq!(detect_url("https://example.com"), None);
}

/// formats 的样例文件经过真实插件规范化后，没有「未知卡池类型」的失败。
#[test]
fn sample_files_normalize_with_real_plugins() {
    let reg = registry();
    for (file, game) in [
        ("uigf-v4.json", "genshin"),
        ("uigf-v3.json", "genshin"),
        ("srgf-v1.json", "starrail"),
        ("gnj-1.0.json", "arknights"),
        ("ak-export-arknights.csv", "arknights"),
        ("ak-export-endfield.csv", "endfield"),
    ] {
        let bytes = std::fs::read(sample(file)).unwrap();
        let conv = reg.sniff(&bytes).unwrap();
        let accounts = conv.decode(&mut bytes.as_slice()).unwrap();
        assert!(accounts.iter().any(|a| a.game == game), "{file}");
        let n = normalize(&reg, accounts, i64::MAX / 2);
        assert!(!n.imports.is_empty(), "{file}");
        for f in &n.failures {
            assert!(!f.reason.starts_with("unknown pool type"), "{file}: {f:?}");
        }
    }
}

fn record(pool: &str, name: &str, rarity: u8, at: i64) -> GachaRecord {
    GachaRecord {
        id: format!("r{at}").into(),
        account_id: "a".into(),
        data: NewRecord {
            pool_type: pool.into(),
            banner_id: Some(format!("b{}", at / 1000)),
            ext_id: None,
            item_id: None,
            item_name: name.into(),
            item_type: None,
            rarity,
            pulled_at: at,
            seq_in_sec: 0,
            is_pull: true,
            raw_json: "{}".into(),
        },
        batch_id: None,
    }
}

/// 原神武器池按记录时间选用当时的命定值上限：2.0 之前没有，5.0 之前上限 2，之后上限 1。
#[test]
fn genshin_epitomized_path_follows_version() {
    let plugin = Genshin::default();
    let weapon = PoolTypeId::from("weapon");
    let max_at = |t| {
        rule_at(plugin.rules(), &weapon, Some(t))
            .unwrap()
            .path_max()
    };
    assert_eq!(max_at(genshin::V2_0 - 1), None);
    assert_eq!(max_at(genshin::V5_0 - 1), Some(2));
    assert_eq!(max_at(genshin::V5_0), Some(1));

    let meta = GameMeta {
        standard: vec!["天空之刃".into()],
        ..GameMeta::default()
    };
    let at = genshin::V5_0 + 10_000;
    let records = vec![
        record("weapon", "天空之刃", 5, at),
        record("weapon", "雾切之回光", 5, at + 1),
        record("weapon", "天空之刃", 5, at + 2),
    ];
    let s = account_stats(&plugin, &meta, &records);
    let w = s.pools.iter().find(|p| p.pool_type == "weapon").unwrap();
    // 第二只出在大保底里，不计入歪率分母
    assert_eq!(w.loss.unwrap().total, 2);
    assert_eq!(w.pity.path_points, Some(1));
    assert!(w.pity.guaranteed);
    assert_eq!(s.pools.len(), 5);
}
