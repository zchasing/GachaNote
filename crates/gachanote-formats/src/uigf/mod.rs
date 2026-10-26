//! UIGF：米哈游三款的通用抽卡记录交换格式。
//!
//! - v4.x（[`UigfV4`]）：一个文件可含原神 `hk4e`、崩铁 `hkrpg`、绝区零 `nap` 多个账号，可导入导出；
//!   导出版本为 v4.2。`hk4e_ugc`（千星奇域）暂不支持，导入时忽略。
//! - v2.x（[`UigfV2`]）、v3.0（[`UigfV3`]）与 SRGF v1.0（[`Srgf`]）：只导入。
//!
//! 时间字段是服务器本地时间，按账号时区换算成 UTC 存储；导出时再换算回去。

mod legacy;

pub use legacy::{Srgf, UigfV2, UigfV3};

use std::collections::BTreeMap;
use std::io::{Read, Write};

use gachanote_core::exchange::{
    ExportAccount, ImportedAccount, ImportedRecord, PoolRef, RecordFailure,
};
use gachanote_core::plugin::Converter;
use gachanote_core::{AppError, FormatId, GameId, Result};
use serde_json::{json, Map, Value};

use crate::util::{
    local_to_utc_ms, now_secs, object, raw_object, read_json, sniff_json_string, text,
    utc_ms_to_local, write_json, APP_VERSION, EXPORT_APP,
};

/// 导出时写入的 UIGF 版本。
pub const EXPORT_VERSION: &str = "v4.2";

/// UIGF 中的游戏键与本软件游戏 ID 的对应关系。
const GAMES: [(&str, &str); 3] = [
    ("hk4e", GameId::GENSHIN),
    ("hkrpg", GameId::STARRAIL),
    ("nap", GameId::ZZZ),
];

/// 记录中按规范输出的字段（其余原始字段导出时丢弃，保证通过 UIGF Schema 校验）。
fn record_fields(key: &str) -> &'static [&'static str] {
    match key {
        "hk4e" => &[
            "uigf_gacha_type",
            "gacha_type",
            "item_id",
            "count",
            "time",
            "name",
            "item_type",
            "rank_type",
            "id",
        ],
        _ => &[
            "gacha_id",
            "gacha_type",
            "item_id",
            "count",
            "time",
            "name",
            "item_type",
            "rank_type",
            "id",
        ],
    }
}

/// 原神 `gacha_type` → `uigf_gacha_type`：两个角色池共享保底，400 归入 301。
pub fn uigf_gacha_type(gacha_type: &str) -> &str {
    if gacha_type == "400" {
        "301"
    } else {
        gacha_type
    }
}

/// 解码一条米哈游记录（UIGF 各版本、SRGF 字段名相同）。
pub(crate) fn decode_record(
    item: &Value,
    location: String,
    tz_offset_min: i32,
    banner_key: Option<&str>,
) -> std::result::Result<ImportedRecord, RecordFailure> {
    let fail = |reason: &str| RecordFailure::new(location.clone(), reason);
    let obj = item.as_object().ok_or_else(|| fail("expected an object"))?;
    let gacha_type = text(obj, "gacha_type")
        .or_else(|| text(obj, "uigf_gacha_type"))
        .ok_or_else(|| fail("missing gacha_type"))?;
    let time = text(obj, "time").ok_or_else(|| fail("missing time"))?;
    let pulled_at = local_to_utc_ms(&time, tz_offset_min).ok_or_else(|| fail("invalid time"))?;
    let rarity = text(obj, "rank_type")
        .ok_or_else(|| fail("missing rank_type"))?
        .parse::<u8>()
        .map_err(|_| fail("invalid rank_type"))?;
    let item_id = text(obj, "item_id");
    // v4 中 name 可省略：没有名称时暂用物品 ID，界面再按元数据包显示名称
    let item_name = text(obj, "name")
        .or_else(|| item_id.clone())
        .ok_or_else(|| fail("missing name and item_id"))?;
    Ok(ImportedRecord {
        pool: PoolRef::Official(gacha_type),
        banner_id: banner_key.and_then(|k| text(obj, k)),
        ext_id: text(obj, "id"),
        item_id,
        item_name,
        item_type: text(obj, "item_type"),
        rarity,
        pulled_at,
        seq: None,
        is_pull: true,
        raw: item.clone(),
        location,
    })
}

/// 解码 `list` 数组到账号。
pub(crate) fn decode_list(
    acc: &mut ImportedAccount,
    list: &Value,
    path: &str,
    banner_key: Option<&str>,
) -> Result<()> {
    let list = list
        .as_array()
        .ok_or_else(|| AppError::parse(path, "expected an array"))?;
    for (i, item) in list.iter().enumerate() {
        match decode_record(item, format!("{path}[{i}]"), acc.tz_offset_min, banner_key) {
            Ok(r) => acc.records.push(r),
            Err(f) => acc.failures.push(f),
        }
    }
    Ok(())
}

/// UIGF v4.x。
#[derive(Debug, Clone, Copy, Default)]
pub struct UigfV4;

impl Converter for UigfV4 {
    fn format(&self) -> FormatId {
        FormatId::UigfV4
    }

    fn sniff(&self, head: &[u8]) -> bool {
        sniff_json_string(head, "version").is_some_and(|v| v.starts_with("v4."))
    }

    fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
        let doc = read_json(input)?;
        let root = object(&doc, "$")?;
        let info = object(root.get("info").unwrap_or(&Value::Null), "$.info")?;
        let version =
            text(info, "version").ok_or_else(|| AppError::parse("$.info.version", "missing"))?;
        if !version.starts_with("v4.") {
            return Err(AppError::parse(
                "$.info.version",
                format!("unsupported version {version}"),
            ));
        }
        let mut out = Vec::new();
        for (key, game) in GAMES {
            let Some(entries) = root.get(key) else {
                continue;
            };
            let entries = entries
                .as_array()
                .ok_or_else(|| AppError::parse(format!("$.{key}"), "expected an array"))?;
            for (i, entry) in entries.iter().enumerate() {
                let path = format!("$.{key}[{i}]");
                let e = object(entry, &path)?;
                let uid = text(e, "uid")
                    .ok_or_else(|| AppError::parse(format!("{path}.uid"), "missing"))?;
                let tz = crate::util::int(e, "timezone")
                    .ok_or_else(|| AppError::parse(format!("{path}.timezone"), "missing"))?;
                let tz = i32::try_from(tz)
                    .ok()
                    .and_then(|h| h.checked_mul(60))
                    .ok_or_else(|| AppError::parse(format!("{path}.timezone"), "out of range"))?;
                let mut acc = ImportedAccount::new(game, uid, tz);
                acc.lang = text(e, "lang");
                let list = e.get("list").unwrap_or(&Value::Null);
                let banner_key = (key != "hk4e").then_some("gacha_id");
                decode_list(&mut acc, list, &format!("{path}.list"), banner_key)?;
                out.push(acc);
            }
        }
        Ok(out)
    }

    fn encode(&self, data: &[ExportAccount], out: &mut dyn Write) -> Result<()> {
        let mut groups: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
        for acc in data {
            let key = GAMES
                .iter()
                .find(|(_, g)| acc.account.game == *g)
                .map(|(k, _)| *k)
                .ok_or_else(|| {
                    AppError::Invalid(format!("UIGF does not support {}", acc.account.game))
                })?;
            groups
                .entry(key)
                .or_default()
                .push(encode_account(key, acc)?);
        }
        let mut root = Map::new();
        root.insert(
            "info".into(),
            json!({
                "export_timestamp": now_secs(),
                "export_app": EXPORT_APP,
                "export_app_version": APP_VERSION,
                "version": EXPORT_VERSION,
            }),
        );
        for (key, accounts) in groups {
            root.insert(key.into(), Value::Array(accounts));
        }
        write_json(out, &Value::Object(root))
    }
}

fn encode_account(key: &str, acc: &ExportAccount) -> Result<Value> {
    let a = &acc.account;
    let tz = a.tz_offset_min;
    let mut list = Vec::with_capacity(acc.records.len());
    for r in &acc.records {
        let raw = raw_object(r);
        let gacha_type = text(&raw, "gacha_type")
            .or_else(|| text(&raw, "uigf_gacha_type"))
            .ok_or_else(|| AppError::Invalid(format!("record {} has no gacha_type", r.id)))?;
        let id = r
            .data
            .ext_id
            .clone()
            .ok_or_else(|| AppError::Invalid(format!("record {} has no official id", r.id)))?;
        let mut item = Map::new();
        for field in record_fields(key) {
            let v = match *field {
                "uigf_gacha_type" => Some(uigf_gacha_type(&gacha_type).to_owned()),
                "gacha_type" => Some(gacha_type.clone()),
                "gacha_id" => text(&raw, "gacha_id").or_else(|| r.data.banner_id.clone()),
                "item_id" => r.data.item_id.clone().or_else(|| Some(String::new())),
                "count" => text(&raw, "count").or_else(|| Some("1".into())),
                "time" => Some(utc_ms_to_local(r.data.pulled_at, tz)),
                "name" => Some(r.data.item_name.clone()),
                "item_type" => r.data.item_type.clone(),
                "rank_type" => Some(r.data.rarity.to_string()),
                "id" => Some(id.clone()),
                _ => None,
            };
            if let Some(v) = v {
                item.insert((*field).to_owned(), Value::String(v));
            }
        }
        list.push(Value::Object(item));
    }
    let lang = acc
        .records
        .iter()
        .find_map(|r| text(&raw_object(r), "lang"))
        .unwrap_or_else(|| "zh-cn".into());
    Ok(json!({
        "uid": a.uid,
        "timezone": tz / 60,
        "lang": lang,
        "list": list,
    }))
}
