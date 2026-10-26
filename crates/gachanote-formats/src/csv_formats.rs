//! CSV 格式：arknights-gacha-export 导出的 CSV（只导入）与本软件自有 CSV（导入导出）。

use std::collections::BTreeMap;
use std::io::{Read, Write};

use gachanote_core::exchange::{
    ExportAccount, ImportedAccount, ImportedRecord, PoolRef, RecordFailure,
};
use gachanote_core::model::DEFAULT_TZ_OFFSET_MIN;
use gachanote_core::plugin::Converter;
use gachanote_core::{AppError, FormatId, GameId, Result};
use serde_json::{json, Map, Value};

use crate::util::{local_to_utc_ms, strip_bom, utc_ms_to_local_millis};

/// arknights-gacha-export 明日方舟 CSV 的表头。
pub const AK_HEADER: &str = "uid,nickName,poolId,poolName,charId,charName,rarity,isNew,gachaTs,pos";
/// arknights-gacha-export 终末地 CSV 的表头。
pub const EF_HEADER: &str =
    "uid,nickName,type,kind,poolId,poolName,itemId,itemName,itemType,rarity,isNew,isFree,gachaTs,seqId,poolVersion,rawJson";
/// 自有 CSV 的表头。
pub const GN_HEADER: &str =
    "game,uid,pool_type,pool_id,id,item_id,item_name,item_type,rarity,time,timezone,seq,is_pull";

fn first_line(head: &[u8]) -> String {
    let head = strip_bom(head);
    let line = head.split(|b| *b == b'\n').next().unwrap_or_default();
    String::from_utf8_lossy(line)
        .trim_end_matches('\r')
        .to_owned()
}

/// 一行 CSV，按表头取列。
struct Row<'a> {
    headers: &'a csv::StringRecord,
    rec: csv::StringRecord,
    line: u64,
}

impl Row<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        let i = self.headers.iter().position(|h| h == name)?;
        self.rec.get(i).map(str::trim).filter(|s| !s.is_empty())
    }

    fn location(&self) -> String {
        format!("line {}", self.line)
    }

    fn fail(&self, reason: impl ToString) -> RecordFailure {
        RecordFailure::new(self.location(), reason)
    }

    fn require(&self, name: &str) -> std::result::Result<&str, RecordFailure> {
        self.get(name)
            .ok_or_else(|| self.fail(format!("missing {name}")))
    }

    fn parse<T: std::str::FromStr>(&self, name: &str) -> std::result::Result<T, RecordFailure> {
        self.require(name)?
            .parse()
            .map_err(|_| self.fail(format!("invalid {name}")))
    }

    /// 整行转成 JSON 对象，作为没有原始 JSON 时的 `raw`。
    fn as_object(&self) -> Value {
        let mut m = Map::new();
        for (h, v) in self.headers.iter().zip(self.rec.iter()) {
            m.insert(h.to_owned(), Value::String(v.to_owned()));
        }
        Value::Object(m)
    }
}

/// 逐行读取 CSV；`f` 返回该行所属账号的键与解码结果。
fn read_rows(
    input: &mut dyn Read,
    mut f: impl FnMut(
        &Row<'_>,
    ) -> (
        Option<(String, String)>,
        std::result::Result<ImportedRecord, RecordFailure>,
    ),
    mut new_account: impl FnMut(&str, &str, &Row<'_>) -> ImportedAccount,
) -> Result<Vec<ImportedAccount>> {
    let mut buf = Vec::new();
    input.read_to_end(&mut buf)?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(strip_bom(&buf));
    let headers = reader
        .headers()
        .map_err(|e| AppError::parse("line 1", e))?
        .clone();
    let mut accounts: BTreeMap<(String, String), ImportedAccount> = BTreeMap::new();
    let mut order = Vec::new();
    for (i, rec) in reader.records().enumerate() {
        let line = i as u64 + 2;
        let rec = rec.map_err(|e| AppError::parse(format!("line {line}"), e))?;
        let row = Row {
            headers: &headers,
            rec,
            line,
        };
        let (key, result) = f(&row);
        let Some(key) = key else {
            return Err(AppError::parse(row.location(), "missing game or uid"));
        };
        let acc = accounts.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            new_account(&key.0, &key.1, &row)
        });
        match result {
            Ok(r) => acc.records.push(r),
            Err(e) => acc.failures.push(e),
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|k| accounts.remove(&k))
        .collect())
}

/// arknights-gacha-export 导出的 CSV（明日方舟、终末地，按表头区分）。
#[derive(Debug, Clone, Copy, Default)]
pub struct AkExportCsv;

fn bool_col(row: &Row<'_>, name: &str) -> Option<bool> {
    match row.get(name)? {
        "true" | "TRUE" | "1" => Some(true),
        "false" | "FALSE" | "0" => Some(false),
        _ => None,
    }
}

fn ak_record(row: &Row<'_>) -> std::result::Result<ImportedRecord, RecordFailure> {
    let pool_id = row.require("poolId")?.to_owned();
    // 接口返回的 rarity 为 0–5，对应 1–6 星
    let rarity = row
        .parse::<u8>("rarity")?
        .checked_add(1)
        .ok_or_else(|| row.fail("invalid rarity"))?;
    let pos: u32 = row.parse("pos")?;
    let mut raw = Map::new();
    raw.insert("poolId".into(), json!(pool_id));
    raw.insert("poolName".into(), json!(row.get("poolName")));
    raw.insert("charId".into(), json!(row.get("charId")));
    raw.insert("charName".into(), json!(row.get("charName")));
    raw.insert("rarity".into(), json!(rarity - 1));
    raw.insert("isNew".into(), json!(bool_col(row, "isNew")));
    raw.insert("gachaTs".into(), json!(row.get("gachaTs")));
    raw.insert("pos".into(), json!(pos));
    Ok(ImportedRecord {
        pool: PoolRef::Official(pool_id.clone()),
        banner_id: Some(pool_id),
        ext_id: None,
        item_id: row.get("charId").map(str::to_owned),
        item_name: row.require("charName")?.to_owned(),
        item_type: None,
        rarity,
        pulled_at: row.parse("gachaTs")?,
        seq: Some(pos),
        is_pull: true,
        raw: Value::Object(raw),
        location: row.location(),
    })
}

fn ef_record(row: &Row<'_>) -> std::result::Result<ImportedRecord, RecordFailure> {
    let kind_of = row.require("type")?.to_owned(); // char | weapon
    let kind = row.get("kind").unwrap_or("draw").to_owned();
    let pool_id = row.require("poolId")?.to_owned();
    let seq_id = row.require("seqId")?.to_owned();
    let raw = match row.get("rawJson").map(serde_json::from_str::<Value>) {
        Some(Ok(v @ Value::Object(_))) => v,
        Some(_) => return Err(row.fail("invalid rawJson")),
        None => row.as_object(),
    };
    let is_draw = kind == "draw";
    // 赠送条目没有星级，记为 1 星且不计抽数
    let rarity = if is_draw {
        row.parse::<u8>("rarity")?
    } else {
        1
    };
    let is_free = bool_col(row, "isFree").unwrap_or(false);
    Ok(ImportedRecord {
        pool: PoolRef::Official(format!("{kind_of}:{pool_id}")),
        banner_id: Some(pool_id.clone()),
        ext_id: Some(format!("{pool_id}:{kind}:{seq_id}")),
        item_id: row.get("itemId").map(str::to_owned),
        item_name: row.require("itemName")?.to_owned(),
        item_type: row.get("itemType").map(str::to_owned),
        rarity,
        pulled_at: row.parse("gachaTs")?,
        seq: None,
        is_pull: is_draw && !is_free,
        raw,
        location: row.location(),
    })
}

impl Converter for AkExportCsv {
    fn format(&self) -> FormatId {
        FormatId::AkExportCsv
    }

    fn sniff(&self, head: &[u8]) -> bool {
        let line = first_line(head);
        line == AK_HEADER || line == EF_HEADER
    }

    fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
        let mut buf = Vec::new();
        input.read_to_end(&mut buf)?;
        let endfield = first_line(&buf) == EF_HEADER;
        let game = if endfield {
            GameId::ENDFIELD
        } else {
            GameId::ARKNIGHTS
        };
        read_rows(
            &mut buf.as_slice(),
            |row| {
                let key = row.get("uid").map(|u| (game.to_owned(), u.to_owned()));
                let rec = if endfield {
                    ef_record(row)
                } else {
                    ak_record(row)
                };
                (key, rec)
            },
            |game, uid, row| {
                let mut acc = ImportedAccount::new(game, uid, DEFAULT_TZ_OFFSET_MIN);
                acc.region = Some("cn".into());
                acc.nickname = row.get("nickName").map(str::to_owned);
                acc
            },
        )
    }

    fn encode(&self, _data: &[ExportAccount], _out: &mut dyn Write) -> Result<()> {
        Err(AppError::Invalid("ak_export_csv is import only".into()))
    }
}

/// 本软件导出的 CSV：方便在 Excel 里查看；不含原始字段，完整备份请用 UIGF / GNJ。
#[derive(Debug, Clone, Copy, Default)]
pub struct GnCsv;

fn gn_record(
    row: &Row<'_>,
    tz_offset_min: i32,
) -> std::result::Result<ImportedRecord, RecordFailure> {
    let pulled_at = local_to_utc_ms(row.require("time")?, tz_offset_min)
        .ok_or_else(|| row.fail("invalid time"))?;
    let is_pull = match row.get("is_pull") {
        None | Some("true") => true,
        Some("false") => false,
        Some(_) => return Err(row.fail("invalid is_pull")),
    };
    Ok(ImportedRecord {
        pool: PoolRef::Normalized(row.require("pool_type")?.into()),
        banner_id: row.get("pool_id").map(str::to_owned),
        ext_id: row.get("id").map(str::to_owned),
        item_id: row.get("item_id").map(str::to_owned),
        item_name: row.require("item_name")?.to_owned(),
        item_type: row.get("item_type").map(str::to_owned),
        rarity: row.parse("rarity")?,
        pulled_at,
        seq: row.get("seq").map(|_| row.parse("seq")).transpose()?,
        is_pull,
        raw: row.as_object(),
        location: row.location(),
    })
}

fn tz_of(row: &Row<'_>) -> i32 {
    row.get("timezone")
        .and_then(|h| h.parse::<i32>().ok())
        .and_then(|h| h.checked_mul(60))
        .unwrap_or(DEFAULT_TZ_OFFSET_MIN)
}

impl Converter for GnCsv {
    fn format(&self) -> FormatId {
        FormatId::GnCsv
    }

    fn sniff(&self, head: &[u8]) -> bool {
        first_line(head) == GN_HEADER
    }

    fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
        read_rows(
            input,
            |row| {
                let key = row
                    .get("game")
                    .zip(row.get("uid"))
                    .map(|(g, u)| (g.to_owned(), u.to_owned()));
                (key, gn_record(row, tz_of(row)))
            },
            |game, uid, row| ImportedAccount::new(game, uid, tz_of(row)),
        )
    }

    fn encode(&self, data: &[ExportAccount], out: &mut dyn Write) -> Result<()> {
        let mut w = csv::Writer::from_writer(out);
        let io = |e: csv::Error| AppError::Internal(e.to_string());
        w.write_record(GN_HEADER.split(',')).map_err(io)?;
        for acc in data {
            let a = &acc.account;
            let tz = a.tz_offset_min;
            for r in &acc.records {
                let d = &r.data;
                w.write_record([
                    a.game.as_str(),
                    a.uid.as_str(),
                    d.pool_type.as_str(),
                    d.banner_id.as_deref().unwrap_or(""),
                    d.ext_id.as_deref().unwrap_or(""),
                    d.item_id.as_deref().unwrap_or(""),
                    d.item_name.as_str(),
                    d.item_type.as_deref().unwrap_or(""),
                    &d.rarity.to_string(),
                    &utc_ms_to_local_millis(d.pulled_at, tz),
                    &(tz / 60).to_string(),
                    &d.seq_in_sec.to_string(),
                    if d.is_pull { "true" } else { "false" },
                ])
                .map_err(io)?;
            }
        }
        w.flush()?;
        Ok(())
    }
}
