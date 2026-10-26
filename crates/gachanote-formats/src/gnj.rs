//! GNJ（GachaNote JSON）：鹰角两款与鸣潮的完整备份格式（SRS 4.2，JSON Schema 见
//! `docs/schema/gnj-1.0.schema.json`）。
//!
//! 结构仿照 UIGF v4：顶层 `info` + 每款游戏一个账号数组。记录存的是本软件规范化后的字段，
//! 时间为 UTC 毫秒，`raw` 保留原始字段，所以本软件导出的 GNJ 再导入后记录完全一致。

use std::collections::BTreeMap;
use std::io::{Read, Write};

use gachanote_core::exchange::{
    ExportAccount, ImportedAccount, ImportedRecord, PoolRef, RecordFailure,
};
use gachanote_core::plugin::Converter;
use gachanote_core::{AppError, FormatId, GameId, Result};
use serde_json::{json, Map, Value};

use crate::util::{
    int, now_secs, object, read_json, sniff_json_string, text, write_json, APP_VERSION, EXPORT_APP,
};

/// 当前 GNJ 版本。
pub const VERSION: &str = "gnj-1.0";

/// GNJ 顶层键与游戏 ID 不同的情况。
const KEY_ALIASES: [(&str, &str); 1] = [("wuthering_waves", GameId::WUWA)];

fn game_of_key(key: &str) -> GameId {
    KEY_ALIASES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or_else(|| GameId::from(key), |(_, g)| GameId::from(*g))
}

fn key_of_game(game: &GameId) -> String {
    KEY_ALIASES
        .iter()
        .find(|(_, g)| game == g)
        .map_or_else(|| game.to_string(), |(k, _)| (*k).to_owned())
}

/// GNJ v1。
#[derive(Debug, Clone, Copy, Default)]
pub struct Gnj;

fn decode_record(
    item: &Value,
    location: String,
) -> std::result::Result<ImportedRecord, RecordFailure> {
    let fail = |reason: &str| RecordFailure::new(location.clone(), reason);
    let o = item.as_object().ok_or_else(|| fail("expected an object"))?;
    let pool_type = text(o, "pool_type").ok_or_else(|| fail("missing pool_type"))?;
    let item_name = text(o, "item_name").ok_or_else(|| fail("missing item_name"))?;
    let rarity = int(o, "rarity")
        .and_then(|r| u8::try_from(r).ok())
        .ok_or_else(|| fail("missing or invalid rarity"))?;
    let pulled_at = int(o, "time").ok_or_else(|| fail("missing or invalid time"))?;
    let seq = match o.get("seq") {
        None | Some(Value::Null) => None,
        Some(_) => Some(
            int(o, "seq")
                .and_then(|s| u32::try_from(s).ok())
                .ok_or_else(|| fail("invalid seq"))?,
        ),
    };
    let is_pull = match o.get("is_pull") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(fail("invalid is_pull")),
    };
    Ok(ImportedRecord {
        pool: PoolRef::Normalized(pool_type.into()),
        banner_id: text(o, "pool_id"),
        ext_id: text(o, "id"),
        item_id: text(o, "item_id"),
        item_name,
        item_type: text(o, "item_type"),
        rarity,
        pulled_at,
        seq,
        is_pull,
        raw: o.get("raw").cloned().unwrap_or_else(|| json!({})),
        location,
    })
}

impl Converter for Gnj {
    fn format(&self) -> FormatId {
        FormatId::Gnj
    }

    fn sniff(&self, head: &[u8]) -> bool {
        sniff_json_string(head, "version").is_some_and(|v| v.starts_with("gnj-"))
    }

    fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
        let doc = read_json(input)?;
        let root = object(&doc, "$")?;
        let info = object(root.get("info").unwrap_or(&Value::Null), "$.info")?;
        let version =
            text(info, "version").ok_or_else(|| AppError::parse("$.info.version", "missing"))?;
        if !version.starts_with("gnj-1.") {
            return Err(AppError::parse(
                "$.info.version",
                format!("unsupported version {version}"),
            ));
        }
        let mut out = Vec::new();
        for (key, entries) in root.iter().filter(|(k, _)| k.as_str() != "info") {
            let entries = entries
                .as_array()
                .ok_or_else(|| AppError::parse(format!("$.{key}"), "expected an array"))?;
            for (i, entry) in entries.iter().enumerate() {
                let path = format!("$.{key}[{i}]");
                let e = object(entry, &path)?;
                let uid = text(e, "uid")
                    .ok_or_else(|| AppError::parse(format!("{path}.uid"), "missing"))?;
                let tz = int(e, "timezone")
                    .and_then(|h| i32::try_from(h).ok())
                    .and_then(|h| h.checked_mul(60))
                    .ok_or_else(|| {
                        AppError::parse(format!("{path}.timezone"), "missing or invalid")
                    })?;
                let mut acc = ImportedAccount::new(game_of_key(key), uid, tz);
                acc.region = text(e, "server");
                acc.nickname = text(e, "nickname");
                acc.lang = text(e, "lang");
                let list = e
                    .get("list")
                    .and_then(Value::as_array)
                    .ok_or_else(|| AppError::parse(format!("{path}.list"), "expected an array"))?;
                for (j, item) in list.iter().enumerate() {
                    match decode_record(item, format!("{path}.list[{j}]")) {
                        Ok(r) => acc.records.push(r),
                        Err(f) => acc.failures.push(f),
                    }
                }
                out.push(acc);
            }
        }
        Ok(out)
    }

    fn encode(&self, data: &[ExportAccount], out: &mut dyn Write) -> Result<()> {
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for acc in data {
            groups
                .entry(key_of_game(&acc.account.game))
                .or_default()
                .push(encode_account(acc)?);
        }
        let mut root = Map::new();
        root.insert(
            "info".into(),
            json!({
                "export_app": EXPORT_APP,
                "export_app_version": APP_VERSION,
                "export_timestamp": now_secs(),
                "version": VERSION,
            }),
        );
        for (key, accounts) in groups {
            root.insert(key, Value::Array(accounts));
        }
        write_json(out, &Value::Object(root))
    }
}

fn encode_account(acc: &ExportAccount) -> Result<Value> {
    let a = &acc.account;
    let list = acc
        .records
        .iter()
        .map(|r| {
            let d = &r.data;
            let raw: Value = serde_json::from_str(&d.raw_json).unwrap_or_else(|_| json!({}));
            let pool_name = raw
                .get("poolName")
                .or_else(|| raw.get("pool_name"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            json!({
                "id": d.ext_id,
                "pool_type": d.pool_type,
                "pool_id": d.banner_id,
                "pool_name": pool_name,
                "item_id": d.item_id,
                "item_name": d.item_name,
                "item_type": d.item_type,
                "rarity": d.rarity,
                "time": d.pulled_at,
                "seq": d.seq_in_sec,
                "is_pull": d.is_pull,
                "raw": raw,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "uid": a.uid,
        "server": a.region,
        "timezone": a.tz_offset_min / 60,
        "nickname": a.nickname,
        "list": list,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_mapping() {
        assert_eq!(game_of_key("wuthering_waves"), "wuwa");
        assert_eq!(game_of_key("arknights"), "arknights");
        assert_eq!(key_of_game(&"wuwa".into()), "wuthering_waves");
        assert_eq!(key_of_game(&"endfield".into()), "endfield");
    }
}
