//! 各格式共用的小工具：时间换算、JSON 取值、格式嗅探。

use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone};
use gachanote_core::exchange::json_text;
use gachanote_core::model::GachaRecord;
use gachanote_core::{AppError, Result};
use serde_json::{Map, Value};

/// 本软件写进导出文件的名称。
pub const EXPORT_APP: &str = "GachaNote";
/// 本软件版本。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 把服务器本地时间 `YYYY-MM-DD HH:MM:SS`（可带毫秒）按 `offset_min` 换算成 UTC 毫秒。
pub fn local_to_utc_ms(s: &str, offset_min: i32) -> Option<i64> {
    let naive = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S%.f").ok()?;
    let tz = FixedOffset::east_opt(offset_min.checked_mul(60)?)?;
    tz.from_local_datetime(&naive)
        .single()
        .map(|t| t.timestamp_millis())
}

fn local(ms: i64, offset_min: i32) -> DateTime<FixedOffset> {
    let utc = FixedOffset::east_opt(0).expect("UTC 偏移合法");
    let tz = offset_min
        .checked_mul(60)
        .and_then(FixedOffset::east_opt)
        .unwrap_or(utc);
    DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .with_timezone(&tz)
}

/// UTC 毫秒 → 服务器本地时间 `YYYY-MM-DD HH:MM:SS`（UIGF 用，精确到秒）。
pub fn utc_ms_to_local(ms: i64, offset_min: i32) -> String {
    local(ms, offset_min)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// UTC 毫秒 → 服务器本地时间 `YYYY-MM-DD HH:MM:SS.mmm`（自有 CSV 用，保留毫秒以便去重）。
pub fn utc_ms_to_local_millis(ms: i64, offset_min: i32) -> String {
    local(ms, offset_min)
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

/// 没有时区字段时按 UID 首位推断（UIGF 规范）：6 → 美服 UTC-5，7 → 欧服 UTC+1，其余 UTC+8。
pub fn infer_tz_hours(uid: &str) -> i32 {
    match uid.chars().next() {
        Some('6') => -5,
        Some('7') => 1,
        _ => 8,
    }
}

/// 取对象中「字符串或数字」字段，空字符串视为缺失。
pub fn text(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).and_then(json_text).filter(|s| !s.is_empty())
}

/// 取整数字段（字符串或数字均可）。
pub fn int(obj: &Map<String, Value>, key: &str) -> Option<i64> {
    match obj.get(key)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// 取对象，否则报解析错误。
pub fn object<'a>(v: &'a Value, location: &str) -> Result<&'a Map<String, Value>> {
    v.as_object()
        .ok_or_else(|| AppError::parse(location, "expected an object"))
}

/// 在文件开头找 JSON 键 `"key"` 对应的字符串值（只用于嗅探，不做完整解析）。
pub fn sniff_json_string(head: &[u8], key: &str) -> Option<String> {
    let s = String::from_utf8_lossy(head);
    let needle = format!("\"{key}\"");
    let mut rest = &s[..];
    while let Some(i) = rest.find(&needle) {
        rest = &rest[i + needle.len()..];
        let after = rest.trim_start();
        if let Some(after) = after.strip_prefix(':') {
            let after = after.trim_start();
            if let Some(v) = after.strip_prefix('"') {
                return v.split('"').next().map(str::to_owned);
            }
        }
    }
    None
}

/// 去掉 UTF-8 BOM。
pub fn strip_bom(head: &[u8]) -> &[u8] {
    head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head)
}

/// 记录的原始字段；解析失败时为空对象。
pub fn raw_object(r: &GachaRecord) -> Map<String, Value> {
    match serde_json::from_str(&r.data.raw_json) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// 把 JSON 写到输出。
pub fn write_json(out: &mut dyn std::io::Write, value: &Value) -> Result<()> {
    serde_json::to_writer_pretty(&mut *out, value)
        .map_err(|e| AppError::Internal(e.to_string()))?;
    out.write_all(b"\n")?;
    Ok(())
}

/// 读入整个 JSON 文档。
pub fn read_json(input: &mut dyn std::io::Read) -> Result<Value> {
    let mut buf = Vec::new();
    input.read_to_end(&mut buf)?;
    Ok(serde_json::from_slice(strip_bom(&buf))?)
}

/// 当前 UNIX 秒。
pub fn now_secs() -> i64 {
    gachanote_core::import::now_ms() / 1000
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn time_conversions() {
        let ms = local_to_utc_ms("2023-04-01 12:00:00", 480).unwrap();
        assert_eq!(ms, 1_680_321_600_000);
        assert_eq!(utc_ms_to_local(ms, 480), "2023-04-01 12:00:00");
        assert_eq!(utc_ms_to_local(ms, -300), "2023-03-31 23:00:00");
        assert_eq!(
            local_to_utc_ms("2023-04-01 12:00:00.250", 0),
            Some(1_680_350_400_250)
        );
        assert_eq!(
            utc_ms_to_local_millis(1_680_350_400_250, 0),
            "2023-04-01 12:00:00.250"
        );
        assert_eq!(local_to_utc_ms("2023/04/01", 480), None);
        assert_eq!(local_to_utc_ms("2023-04-01 12:00:00", 100_000), None);
        // 越界时回退到 UTC
        assert_eq!(utc_ms_to_local(0, 100_000), "1970-01-01 00:00:00");
        assert_eq!(utc_ms_to_local(i64::MAX, 0), "1970-01-01 00:00:00");
    }

    #[test]
    fn tz_inference() {
        assert_eq!(infer_tz_hours("600000001"), -5);
        assert_eq!(infer_tz_hours("700000001"), 1);
        assert_eq!(infer_tz_hours("100000001"), 8);
        assert_eq!(infer_tz_hours(""), 8);
    }

    #[test]
    fn json_helpers() {
        let v = json!({"a": "1", "b": 2, "c": "", "d": " 7 ", "e": [1]});
        let o = object(&v, "$").unwrap();
        assert_eq!(text(o, "a").as_deref(), Some("1"));
        assert_eq!(text(o, "b").as_deref(), Some("2"));
        assert_eq!(text(o, "c"), None);
        assert_eq!(int(o, "d"), Some(7));
        assert_eq!(int(o, "b"), Some(2));
        assert_eq!(int(o, "e"), None);
        assert_eq!(int(o, "zz"), None);
        assert!(object(&json!([]), "$.x")
            .unwrap_err()
            .to_string()
            .contains("$.x"));
    }

    #[test]
    fn sniffing_helpers() {
        let head = br#"{"info": {"uigf_version" :"v3.0", "version":  "v4.1"}}"#;
        assert_eq!(sniff_json_string(head, "version").as_deref(), Some("v4.1"));
        assert_eq!(
            sniff_json_string(head, "uigf_version").as_deref(),
            Some("v3.0")
        );
        assert_eq!(sniff_json_string(br#"{"version": 4}"#, "version"), None);
        assert_eq!(sniff_json_string(b"{}", "version"), None);
        assert_eq!(strip_bom(b"\xEF\xBB\xBFabc"), b"abc");
        assert_eq!(strip_bom(b"abc"), b"abc");
        let mut input: &[u8] = b"\xEF\xBB\xBF{\"a\":1}";
        assert_eq!(read_json(&mut input).unwrap(), json!({"a": 1}));
        assert!(now_secs() > 1_700_000_000);
    }
}
