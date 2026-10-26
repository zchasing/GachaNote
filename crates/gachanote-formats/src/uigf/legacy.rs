//! 只导入的旧格式：UIGF v2.x / v3.0（原神）与 SRGF v1.0（崩铁）。
//!
//! 结构都是 `{ info: { uid, lang, region_time_zone?, *_version }, list: [...] }`。

use std::io::{Read, Write};

use gachanote_core::exchange::{ExportAccount, ImportedAccount};
use gachanote_core::plugin::Converter;
use gachanote_core::{AppError, FormatId, GameId, Result};
use serde_json::Value;

use super::decode_list;
use crate::util::{infer_tz_hours, int, object, read_json, sniff_json_string, text};

fn decode_single(
    input: &mut dyn Read,
    version_key: &str,
    accept: &dyn Fn(&str) -> bool,
    game: &str,
    banner_key: Option<&str>,
) -> Result<Vec<ImportedAccount>> {
    let doc = read_json(input)?;
    let root = object(&doc, "$")?;
    let info = object(root.get("info").unwrap_or(&Value::Null), "$.info")?;
    let version = text(info, version_key)
        .ok_or_else(|| AppError::parse(format!("$.info.{version_key}"), "missing"))?;
    if !accept(&version) {
        return Err(AppError::parse(
            format!("$.info.{version_key}"),
            format!("unsupported version {version}"),
        ));
    }
    let uid = text(info, "uid").ok_or_else(|| AppError::parse("$.info.uid", "missing"))?;
    // 文件给出的时区优先，否则按 UID 推断
    let hours = int(info, "region_time_zone")
        .and_then(|h| i32::try_from(h).ok())
        .unwrap_or_else(|| infer_tz_hours(&uid));
    let mut acc = ImportedAccount::new(game, uid, hours.saturating_mul(60));
    acc.lang = text(info, "lang");
    decode_list(
        &mut acc,
        root.get("list").unwrap_or(&Value::Null),
        "$.list",
        banner_key,
    )?;
    Ok(vec![acc])
}

fn read_only(format: FormatId) -> AppError {
    AppError::Invalid(format!("{format} is import only"))
}

macro_rules! uigf_legacy {
    ($(#[$doc:meta])* $name:ident, $format:expr, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name;

        impl Converter for $name {
            fn format(&self) -> FormatId {
                $format
            }

            fn sniff(&self, head: &[u8]) -> bool {
                sniff_json_string(head, "uigf_version").is_some_and(|v| v.starts_with($prefix))
            }

            fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
                decode_single(input, "uigf_version", &|v| v.starts_with($prefix), GameId::GENSHIN, None)
            }

            fn encode(&self, _data: &[ExportAccount], _out: &mut dyn Write) -> Result<()> {
                Err(read_only(self.format()))
            }
        }
    };
}

uigf_legacy!(
    /// UIGF v3.0（原神）。
    UigfV3,
    FormatId::UigfV3,
    "v3."
);
uigf_legacy!(
    /// UIGF v2.x（原神）。
    UigfV2,
    FormatId::UigfV2,
    "v2."
);

/// SRGF v1.0（崩铁）。
#[derive(Debug, Clone, Copy, Default)]
pub struct Srgf;

fn is_srgf_v1(v: &str) -> bool {
    v.starts_with("v1.")
}

impl Converter for Srgf {
    fn format(&self) -> FormatId {
        FormatId::SrgfV1
    }

    fn sniff(&self, head: &[u8]) -> bool {
        sniff_json_string(head, "srgf_version").is_some_and(|v| is_srgf_v1(&v))
    }

    fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
        decode_single(
            input,
            "srgf_version",
            &is_srgf_v1,
            GameId::STARRAIL,
            Some("gacha_id"),
        )
    }

    fn encode(&self, _data: &[ExportAccount], _out: &mut dyn Write) -> Result<()> {
        Err(read_only(self.format()))
    }
}
