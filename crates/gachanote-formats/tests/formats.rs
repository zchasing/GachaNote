//! 各格式的解码、编码与往返测试。样例文件在 `tests/samples/`（UID 均为虚构）。

use std::path::Path;

use gachanote_core::exchange::{ExportAccount, ImportedAccount, ImportedRecord, PoolRef};
use gachanote_core::model::{Account, GachaRecord, NewRecord};
use gachanote_core::plugin::Converter;
use gachanote_core::{AppError, FormatId};
use gachanote_formats::{all_converters, AkExportCsv, GnCsv, Gnj, Srgf, UigfV2, UigfV3, UigfV4};
use serde_json::{json, Value};

fn sample(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples")
            .join(name),
    )
    .unwrap()
}

fn decode(c: &dyn Converter, bytes: &[u8]) -> gachanote_core::Result<Vec<ImportedAccount>> {
    let mut input = bytes;
    c.decode(&mut input)
}

fn encode(c: &dyn Converter, data: &[ExportAccount]) -> gachanote_core::Result<Vec<u8>> {
    let mut out = Vec::new();
    c.encode(data, &mut out)?;
    Ok(out)
}

/// 模拟入库：卡池类型原样保留，同秒序号取来源序号或 0。
fn to_export(acc: &ImportedAccount) -> ExportAccount {
    let records = acc
        .records
        .iter()
        .enumerate()
        .map(|(i, r)| GachaRecord {
            id: format!("r{i}").into(),
            account_id: "a".into(),
            data: NewRecord {
                pool_type: match &r.pool {
                    PoolRef::Official(c) => c.as_str().into(),
                    PoolRef::Normalized(p) => p.clone(),
                },
                banner_id: r.banner_id.clone(),
                ext_id: r.ext_id.clone(),
                item_id: r.item_id.clone(),
                item_name: r.item_name.clone(),
                item_type: r.item_type.clone(),
                rarity: r.rarity,
                pulled_at: r.pulled_at,
                seq_in_sec: r.seq.unwrap_or(0),
                is_pull: r.is_pull,
                raw_json: r.raw.to_string(),
            },
            batch_id: None,
        })
        .collect();
    ExportAccount {
        account: Account {
            id: "a".into(),
            game: acc.game.clone(),
            uid: acc.uid.clone(),
            region: acc.region.clone(),
            nickname: acc.nickname.clone(),
            tz_offset_min: acc.tz_offset_min,
            created_at: 0,
        },
        records,
    }
}

/// 往返比较用：去掉位置与原始字段。
fn essential(r: &ImportedRecord) -> Value {
    json!([
        r.pool,
        r.banner_id,
        r.ext_id,
        r.item_id,
        r.item_name,
        r.item_type,
        r.rarity,
        r.pulled_at,
        r.is_pull
    ])
}

fn essentials(accounts: &[ImportedAccount]) -> Vec<(String, String, i32, Vec<Value>)> {
    accounts
        .iter()
        .map(|a| {
            (
                a.game.to_string(),
                a.uid.clone(),
                a.tz_offset_min,
                a.records.iter().map(essential).collect(),
            )
        })
        .collect()
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

// ---------- 嗅探 ----------

#[test]
fn each_sample_is_claimed_by_exactly_one_converter() {
    let cases = [
        ("uigf-v4.json", FormatId::UigfV4),
        ("uigf-v3.json", FormatId::UigfV3),
        ("uigf-v2.json", FormatId::UigfV2),
        ("srgf-v1.json", FormatId::SrgfV1),
        ("gnj-1.0.json", FormatId::Gnj),
        ("ak-export-arknights.csv", FormatId::AkExportCsv),
        ("ak-export-endfield.csv", FormatId::AkExportCsv),
    ];
    let converters = all_converters();
    for (file, expected) in cases {
        let head = sample(file);
        let claimed: Vec<_> = converters
            .iter()
            .filter(|c| c.sniff(&head))
            .map(|c| c.format())
            .collect();
        assert_eq!(claimed, [expected], "{file}");
    }
    let gn = gachanote_formats::GN_HEADER.as_bytes();
    let claimed: Vec<_> = converters
        .iter()
        .filter(|c| c.sniff(gn))
        .map(|c| c.format())
        .collect();
    assert_eq!(claimed, [FormatId::GnCsv]);
    assert!(converters.iter().all(|c| !c.sniff(b"hello world")));
}

// ---------- UIGF v4 ----------

#[test]
fn uigf_v4_decodes_all_games() {
    let accounts = decode(&UigfV4, &sample("uigf-v4.json")).unwrap();
    let summary: Vec<_> = accounts
        .iter()
        .map(|a| {
            (
                a.game.as_str(),
                a.uid.as_str(),
                a.tz_offset_min,
                a.records.len(),
                a.failures.len(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("genshin", "100000001", 480, 52, 0),
            ("starrail", "100000002", 480, 2, 0),
            ("zzz", "10000003", 480, 2, 1),
        ]
    );
    let g = &accounts[0];
    assert_eq!(g.lang.as_deref(), Some("zh-cn"));
    let first = &g.records[0];
    assert_eq!(first.pool, PoolRef::Official("301".into()));
    // 2024-01-01 12:00:00 UTC+8
    assert_eq!(first.pulled_at, 1_704_081_600_000);
    assert_eq!(first.ext_id.as_deref(), Some("1704081600000000001"));
    assert_eq!(first.location, "$.hk4e[0].list[0]");
    assert_eq!(g.records[14].pool, PoolRef::Official("400".into()));
    assert_eq!(g.records[14].banner_id, None);
    // 崩铁记录带卡池 ID
    assert_eq!(accounts[1].records[0].banner_id.as_deref(), Some("2003"));
    // 绝区零：缺 name 时暂用 item_id；时间不合法进入失败列表
    let z = &accounts[2];
    assert_eq!(z.records[0].banner_id, None);
    assert_eq!(z.records[1].item_name, "1191");
    assert_eq!(z.failures[0].location, "$.nap[0].list[2]");
    assert_eq!(z.failures[0].reason, "invalid time");
}

#[test]
fn uigf_v4_round_trip_keeps_fields() {
    let accounts = decode(&UigfV4, &sample("uigf-v4.json")).unwrap();
    let export: Vec<_> = accounts.iter().map(to_export).collect();
    let bytes = encode(&UigfV4, &export).unwrap();
    let doc = parse_json(&bytes);
    assert_eq!(doc["info"]["version"], "v4.2");
    assert_eq!(doc["info"]["export_app"], "GachaNote");
    // 400 归入 301
    assert_eq!(doc["hk4e"][0]["list"][14]["uigf_gacha_type"], "301");
    assert_eq!(doc["hk4e"][0]["list"][14]["gacha_type"], "400");
    insta::assert_json_snapshot!("uigf_v4_export", doc, { ".info.export_timestamp" => "[timestamp]" });

    let again = decode(&UigfV4, &bytes).unwrap();
    assert_eq!(essentials(&again), essentials(&accounts));
}

#[test]
fn uigf_v4_encode_rejects_unsupported_data() {
    let ak = decode(&Gnj, &sample("gnj-1.0.json")).unwrap();
    let err = encode(&UigfV4, &[to_export(&ak[0])]).unwrap_err();
    assert_eq!(
        err,
        AppError::Invalid("UIGF does not support arknights".into())
    );

    let mut g = to_export(&decode(&UigfV4, &sample("uigf-v4.json")).unwrap()[0]);
    g.records.truncate(1);
    g.records[0].data.raw_json = "{}".into();
    assert!(encode(&UigfV4, &[g.clone()])
        .unwrap_err()
        .to_string()
        .contains("gacha_type"));
    g.records[0].data.raw_json = r#"{"gacha_type":"301"}"#.into();
    g.records[0].data.ext_id = None;
    assert!(encode(&UigfV4, &[g])
        .unwrap_err()
        .to_string()
        .contains("official id"));
}

#[test]
fn uigf_v4_structural_errors() {
    let cases: [(&str, &str); 8] = [
        ("[]", "$"),
        (r#"{"hk4e": []}"#, "$.info"),
        (r#"{"info": {}}"#, "$.info.version"),
        (
            r#"{"info": {"version": "v3.0"}}"#,
            "unsupported version v3.0",
        ),
        (r#"{"info": {"version": "v4.0"}, "hk4e": {}}"#, "$.hk4e"),
        (
            r#"{"info": {"version": "v4.0"}, "hk4e": [{"timezone": 8, "list": []}]}"#,
            "$.hk4e[0].uid",
        ),
        (
            r#"{"info": {"version": "v4.0"}, "nap": [{"uid": "1", "list": []}]}"#,
            "$.nap[0].timezone",
        ),
        (
            r#"{"info": {"version": "v4.0"}, "hkrpg": [{"uid": "1", "timezone": 8}]}"#,
            "$.hkrpg[0].list",
        ),
    ];
    for (doc, expected) in cases {
        let err = decode(&UigfV4, doc.as_bytes()).unwrap_err();
        assert!(err.to_string().contains(expected), "{doc}: {err}");
    }
    let err = decode(&UigfV4, br#"{"info": {"version": "v4.0"}, "nap": [{"uid": "1", "timezone": 99999999999, "list": []}]}"#).unwrap_err();
    assert!(err.to_string().contains("out of range"));
    assert_eq!(decode(&UigfV4, b"{").unwrap_err().code(), "parse");
}

#[test]
fn uigf_record_level_failures() {
    let doc = json!({"info": {"version": "v4.0"}, "hk4e": [{"uid": "1", "timezone": 8, "list": [
        1,
        {"time": "2024-01-01 00:00:00", "rank_type": "3", "name": "a"},
        {"gacha_type": "301", "rank_type": "3", "name": "a"},
        {"gacha_type": "301", "time": "2024-01-01 00:00:00", "name": "a"},
        {"gacha_type": "301", "time": "2024-01-01 00:00:00", "rank_type": "x", "name": "a"},
        {"gacha_type": "301", "time": "2024-01-01 00:00:00", "rank_type": "3"},
        {"uigf_gacha_type": "301", "time": "2024-01-01 00:00:00", "rank_type": 3, "name": "ok", "id": 5}
    ]}]});
    let accounts = decode(&UigfV4, doc.to_string().as_bytes()).unwrap();
    let reasons: Vec<_> = accounts[0]
        .failures
        .iter()
        .map(|f| f.reason.as_str())
        .collect();
    assert_eq!(
        reasons,
        [
            "expected an object",
            "missing gacha_type",
            "missing time",
            "missing rank_type",
            "invalid rank_type",
            "missing name and item_id"
        ]
    );
    let ok = &accounts[0].records[0];
    assert_eq!(
        (ok.pool.clone(), ok.ext_id.as_deref()),
        (PoolRef::Official("301".into()), Some("5"))
    );
}

// ---------- UIGF v2 / v3 / SRGF ----------

#[test]
fn uigf_v3_and_v2_decode() {
    let v3 = decode(&UigfV3, &sample("uigf-v3.json")).unwrap();
    assert_eq!(v3.len(), 1);
    assert_eq!((v3[0].game.as_str(), v3[0].tz_offset_min), ("genshin", 480));
    assert_eq!(v3[0].records.len(), 2);
    assert_eq!(v3[0].records[1].pool, PoolRef::Official("500".into()));
    assert_eq!(v3[0].records[0].item_id, None);
    assert_eq!(v3[0].failures[0].reason, "missing rank_type");
    assert_eq!(v3[0].failures[0].location, "$.list[2]");

    // v2.2：没有 region_time_zone，按 UID 首位 6 推断为 UTC-5；数字字段也接受
    let v2 = decode(&UigfV2, &sample("uigf-v2.json")).unwrap();
    assert_eq!(v2[0].tz_offset_min, -300);
    assert_eq!(v2[0].lang.as_deref(), Some("en-us"));
    let r = &v2[0].records[0];
    assert_eq!(
        (r.rarity, r.ext_id.as_deref()),
        (4, Some("1640952000000000001"))
    );
    // 2021-12-31 07:00:00 UTC-5 = 12:00 UTC
    assert_eq!(r.pulled_at, 1_640_952_000_000);

    let err = decode(&UigfV3, &sample("uigf-v2.json")).unwrap_err();
    assert!(err.to_string().contains("unsupported version v2.2"));
    let err = decode(
        &UigfV2,
        br#"{"info": {"uigf_version": "v2.2"}, "list": []}"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("$.info.uid"));
    let err = decode(&UigfV2, br#"{"info": {"uid": "1"}, "list": []}"#).unwrap_err();
    assert!(err.to_string().contains("$.info.uigf_version"));
    let err = decode(
        &UigfV2,
        br#"{"info": {"uid": "1", "uigf_version": "v2.0"}, "list": {}}"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("$.list"));
}

#[test]
fn srgf_decode() {
    let a = decode(&Srgf, &sample("srgf-v1.json")).unwrap();
    assert_eq!(
        (a[0].game.as_str(), a[0].uid.as_str()),
        ("starrail", "100000002")
    );
    let pools: Vec<_> = a[0]
        .records
        .iter()
        .map(|r| (r.pool.clone(), r.banner_id.clone()))
        .collect();
    assert_eq!(
        pools,
        [
            (PoolRef::Official("1".into()), Some("1001".into())),
            (PoolRef::Official("11".into()), Some("2001".into()))
        ]
    );
    let err = decode(
        &Srgf,
        br#"{"info": {"uid": "1", "srgf_version": "v2.0"}, "list": []}"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("unsupported"));
}

#[test]
fn import_only_formats_refuse_encode() {
    for c in [&UigfV3 as &dyn Converter, &UigfV2, &Srgf, &AkExportCsv] {
        assert_eq!(
            encode(c, &[]).unwrap_err().code(),
            "invalid",
            "{}",
            c.format()
        );
    }
}

// ---------- GNJ ----------

#[test]
fn gnj_decode() {
    let a = decode(&Gnj, &sample("gnj-1.0.json")).unwrap();
    let summary: Vec<_> = a
        .iter()
        .map(|x| (x.game.as_str(), x.records.len(), x.failures.len()))
        .collect();
    assert_eq!(summary, [("arknights", 2, 1), ("wuwa", 1, 0)]);
    assert_eq!(a[0].nickname.as_deref(), Some("博士#1234"));
    assert_eq!(a[0].region.as_deref(), Some("cn"));
    let r = &a[0].records[1];
    assert_eq!(r.pool, PoolRef::Normalized("limited".into()));
    assert_eq!((r.seq, r.rarity, r.is_pull), (Some(1), 3, true));
    assert_eq!(
        r.raw,
        json!({"poolId": "LIMITED_1", "pos": 1, "poolName": "限定寻访"})
    );
    assert_eq!(a[0].failures[0].reason, "missing or invalid rarity");
    assert_eq!(a[1].records[0].seq, None);
}

#[test]
fn gnj_round_trip_is_exact() {
    let a = decode(&Gnj, &sample("gnj-1.0.json")).unwrap();
    let export: Vec<_> = a.iter().map(to_export).collect();
    let bytes = encode(&Gnj, &export).unwrap();
    let doc = parse_json(&bytes);
    insta::assert_json_snapshot!("gnj_export", doc, { ".info.export_timestamp" => "[timestamp]" });
    let again = decode(&Gnj, &bytes).unwrap();
    let again_export: Vec<_> = again.iter().map(to_export).collect();
    for (x, y) in export.iter().zip(&again_export) {
        assert_eq!(x.account, y.account);
        assert_eq!(x.records, y.records);
    }
}

#[test]
fn gnj_errors() {
    let cases: [(&str, &str); 6] = [
        (
            r#"{"info": {"version": "gnj-2.0"}}"#,
            "unsupported version gnj-2.0",
        ),
        (r#"{"info": {}}"#, "$.info.version"),
        (
            r#"{"info": {"version": "gnj-1.0"}, "arknights": 1}"#,
            "$.arknights",
        ),
        (
            r#"{"info": {"version": "gnj-1.0"}, "arknights": [{"timezone": 8, "list": []}]}"#,
            "$.arknights[0].uid",
        ),
        (
            r#"{"info": {"version": "gnj-1.0"}, "arknights": [{"uid": "1", "list": []}]}"#,
            "timezone",
        ),
        (
            r#"{"info": {"version": "gnj-1.0"}, "arknights": [{"uid": "1", "timezone": 8}]}"#,
            "$.arknights[0].list",
        ),
    ];
    for (doc, expected) in cases {
        let err = decode(&Gnj, doc.as_bytes()).unwrap_err();
        assert!(err.to_string().contains(expected), "{doc}: {err}");
    }
    let doc = json!({"info": {"version": "gnj-1.0"}, "endfield": [{"uid": "1", "timezone": 8, "list": [
        "x",
        {"item_name": "a", "rarity": 3, "time": 1},
        {"pool_type": "p", "rarity": 3, "time": 1},
        {"pool_type": "p", "item_name": "a", "rarity": 3},
        {"pool_type": "p", "item_name": "a", "rarity": 3, "time": 1, "seq": -1},
        {"pool_type": "p", "item_name": "a", "rarity": 3, "time": 1, "is_pull": "yes"},
        {"pool_type": "p", "item_name": "a", "rarity": 3, "time": 1, "is_pull": false, "seq": null}
    ]}]});
    let a = decode(&Gnj, doc.to_string().as_bytes()).unwrap();
    let reasons: Vec<_> = a[0].failures.iter().map(|f| f.reason.as_str()).collect();
    assert_eq!(
        reasons,
        [
            "expected an object",
            "missing pool_type",
            "missing item_name",
            "missing or invalid time",
            "invalid seq",
            "invalid is_pull"
        ]
    );
    assert!(!a[0].records[0].is_pull);
    assert_eq!(a[0].records[0].raw, json!({}));
}

// ---------- arknights-gacha-export CSV ----------

#[test]
fn ak_export_arknights_csv() {
    let a = decode(&AkExportCsv, &sample("ak-export-arknights.csv")).unwrap();
    assert_eq!(a.len(), 2);
    let doctor = &a[0];
    assert_eq!(
        (doctor.game.as_str(), doctor.uid.as_str()),
        ("arknights", "12345678")
    );
    assert_eq!(doctor.nickname.as_deref(), Some("博士"));
    assert_eq!(doctor.region.as_deref(), Some("cn"));
    assert_eq!(doctor.records.len(), 3);
    let r = &doctor.records[0];
    assert_eq!(r.pool, PoolRef::Official("LIMITED_1".into()));
    assert_eq!(r.banner_id.as_deref(), Some("LIMITED_1"));
    // 接口 rarity 5 = 6 星
    assert_eq!((r.rarity, r.seq, r.ext_id.as_deref()), (6, Some(0), None));
    assert_eq!(r.pulled_at, 1_761_292_800_000);
    assert_eq!(r.raw["poolName"], "限定寻访, 春节");
    assert_eq!(r.raw["rarity"], 5);
    assert_eq!(doctor.records[1].seq, Some(1));
    assert_eq!(doctor.failures[0].location, "line 5");
    assert_eq!(doctor.failures[0].reason, "invalid rarity");
    assert_eq!(a[1].nickname, None);
}

#[test]
fn ak_export_endfield_csv() {
    let a = decode(&AkExportCsv, &sample("ak-export-endfield.csv")).unwrap();
    assert_eq!(a.len(), 1);
    let e = &a[0];
    assert_eq!(
        (e.game.as_str(), e.records.len(), e.failures.len()),
        ("endfield", 4, 1)
    );
    let r: Vec<_> = e
        .records
        .iter()
        .map(|r| {
            (
                r.pool.clone(),
                r.ext_id.clone().unwrap(),
                r.rarity,
                r.is_pull,
            )
        })
        .collect();
    assert_eq!(
        r,
        [
            (
                PoolRef::Official("char:special_1_0_1".into()),
                "special_1_0_1:draw:212".into(),
                6,
                true
            ),
            // 免费十连不计抽数
            (
                PoolRef::Official("char:special_1_0_1".into()),
                "special_1_0_1:draw:213".into(),
                4,
                false
            ),
            // 赠送条目：没有星级，不计抽数
            (
                PoolRef::Official("char:special_1_0_1".into()),
                "special_1_0_1:gift_intel_book:214".into(),
                1,
                false
            ),
            (
                PoolRef::Official("weapon:wpn_1".into()),
                "wpn_1:draw:60".into(),
                6,
                true
            ),
        ]
    );
    // rawJson 列完整保留
    assert_eq!(e.records[2].raw["kind"], "gift_intel_book");
    assert_eq!(e.records[3].raw["poolVersion"], Value::Null);
    assert_eq!(e.records[3].item_type.as_deref(), Some("单手剑"));
    assert_eq!(e.failures[0].reason, "invalid rawJson");
}

#[test]
fn csv_row_errors() {
    let header = gachanote_formats::AK_HEADER;
    let err = decode(
        &AkExportCsv,
        format!("{header}\n,x,p,n,c,n,1,false,1,0\n").as_bytes(),
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "parse error at line 2: missing game or uid"
    );
    let a = decode(
        &AkExportCsv,
        format!(
            "{header}\n1,x,,n,c,n,1,false,1,0\n1,x,p,n,c,n,255,false,1,0\n1,x,p,n,c,n,1,false,1,\n"
        )
        .as_bytes(),
    )
    .unwrap();
    let reasons: Vec<_> = a[0].failures.iter().map(|f| f.reason.as_str()).collect();
    assert_eq!(reasons, ["missing poolId", "invalid rarity", "missing pos"]);
    // 没有 rawJson 时用整行作为原始字段
    let ef = gachanote_formats::EF_HEADER;
    let a = decode(
        &AkExportCsv,
        format!("{ef}\n1,,char,,p,n,i,物品,,5,,,100,1,,\n").as_bytes(),
    )
    .unwrap();
    assert_eq!(a[0].records[0].raw["itemName"], "物品");
    assert!(a[0].records[0].is_pull);
    let bad = decode(&AkExportCsv, b"a,b\n\"unterminated\n");
    assert!(bad.is_ok() || bad.unwrap_err().code() == "parse");
}

// ---------- 自有 CSV ----------

#[test]
fn gn_csv_round_trip() {
    let mut accounts = decode(&AkExportCsv, &sample("ak-export-arknights.csv")).unwrap();
    accounts.extend(decode(&AkExportCsv, &sample("ak-export-endfield.csv")).unwrap());
    let mut genshin = decode(&UigfV3, &sample("uigf-v3.json")).unwrap();
    genshin[0].tz_offset_min = -300;
    accounts.extend(genshin);
    let export: Vec<_> = accounts.iter().map(to_export).collect();
    let bytes = encode(&GnCsv, &export).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    insta::assert_snapshot!("gn_csv_export", text);

    let again = decode(&GnCsv, &bytes).unwrap();
    assert_eq!(again.len(), accounts.len());
    for (x, y) in export.iter().zip(again.iter().map(to_export)) {
        assert_eq!(
            (&x.account.game, &x.account.uid, x.account.tz_offset_min),
            (&y.account.game, &y.account.uid, y.account.tz_offset_min)
        );
        assert_eq!(x.records.len(), y.records.len());
        for (a, b) in x.records.iter().zip(&y.records) {
            let mut b = b.data.clone();
            b.raw_json = a.data.raw_json.clone();
            assert_eq!(a.data, b);
        }
    }
}

#[test]
fn gn_csv_row_errors() {
    let h = gachanote_formats::GN_HEADER;
    let csv = format!(
        "{h}\n\
         arknights,1,p,,,,a,,6,2025-01-01 00:00:00,8,0,maybe\n\
         arknights,1,p,,,,a,,6,yesterday,8,0,true\n\
         arknights,1,p,,,,a,,6,2025-01-01 00:00:00.5,x,,\n"
    );
    let a = decode(&GnCsv, csv.as_bytes()).unwrap();
    let reasons: Vec<_> = a[0].failures.iter().map(|f| f.reason.as_str()).collect();
    assert_eq!(reasons, ["invalid is_pull", "invalid time"]);
    // 时区列不合法时取默认 UTC+8；is_pull 为空视为计抽数
    let r = &a[0].records[0];
    assert_eq!(
        (r.pulled_at, r.is_pull, r.seq),
        (1_735_660_800_500, true, None)
    );
    let err = decode(
        &GnCsv,
        format!("{h}\n,1,p,,,,a,,6,2025-01-01 00:00:00,8,0,true\n").as_bytes(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("missing game or uid"));
}
