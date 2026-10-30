//! 元数据包的文件格式（SRS 4.3，JSON Schema 见 `metadata/schema/`）。
//!
//! - `manifest.json`：包版本、最低应用版本、各文件的 SHA-256；
//! - `<game>.json`：物品表（ID、星级、多语言名称）、常驻名单、历史卡池（起止时间、UP 名单）。
//!
//! 名单里写物品 ID 或名称；转换成 [`GameMeta`] 时展开成「ID + 各语言名称」，
//! 这样不论记录里是 ID 还是哪种语言的名称都能匹配。

use std::collections::BTreeMap;

use chrono::DateTime;
use gachanote_core::meta::{Banner, GameMeta};
use gachanote_core::{AppError, GameId, PoolTypeId, Result};
use serde::{Deserialize, Serialize};

/// `manifest.json`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// 包版本，如 `2026.10.26`；按数字逐段比较。
    pub version: String,
    /// 使用本包所需的最低应用版本。
    #[serde(default)]
    pub min_app_version: Option<String>,
    /// 包内文件。
    pub files: Vec<ManifestFile>,
}

/// manifest 中的一个文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    /// 相对 `metadata/` 的路径。
    pub path: String,
    /// 文件内容的 SHA-256（小写十六进制）。
    pub sha256: String,
}

/// 一个物品。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackItem {
    /// 物品 ID（与记录中的 `item_id` 相同；没有官方 ID 的游戏用名称）。
    pub id: String,
    /// 星级（按游戏内显示）。
    pub rarity: u8,
    /// 各语言名称，键为 `zh-cn`、`en-us` 等。
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    /// 头像图标的 SHA-256（小写十六进制），文件在 `icons/<game>/<sha256>.webp`；没有头像时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl PackItem {
    /// 物品 ID 或任一语言的名称是否等于 `key`。
    pub fn matches(&self, key: &str) -> bool {
        self.id == key || self.names.values().any(|n| n == key)
    }
}

/// 是否为合法的 SHA-256 十六进制串（小写）。
pub fn is_sha256(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 一期卡池。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackBanner {
    /// 卡池 ID（包内唯一；记录自带卡池 ID 的游戏与之相同）。
    pub id: String,
    /// 本软件的卡池类型。
    pub pool_type: String,
    /// 各语言卡池名，可空。
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    /// 游戏版本号，如 `4.2`，可空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// 开始时间（RFC 3339，含时区）。
    pub start: String,
    /// 结束时间（RFC 3339，含时区）。
    pub end: String,
    /// UP 最高星（物品 ID 或名称）。
    #[serde(default)]
    pub up_top: Vec<String>,
    /// UP 次高星。
    #[serde(default)]
    pub up_second: Vec<String>,
}

/// `<game>.json`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GamePack {
    /// 游戏。
    pub game: String,
    /// 物品表。
    #[serde(default)]
    pub items: Vec<PackItem>,
    /// 常驻最高星（物品 ID 或名称）。
    #[serde(default)]
    pub standard: Vec<String>,
    /// 历史卡池。
    #[serde(default)]
    pub banners: Vec<PackBanner>,
}

fn time_ms(s: &str, location: &str) -> Result<i64> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.timestamp_millis())
        .map_err(|e| AppError::parse(location, e))
}

impl GamePack {
    /// 解析并检查：时间合法且开始早于结束、卡池 ID 不重复、图标哈希格式正确。
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let pack: Self = serde_json::from_slice(bytes)?;
        pack.to_game_meta()?;
        if let Some(i) = pack
            .items
            .iter()
            .find(|i| i.icon.as_deref().is_some_and(|h| !is_sha256(h)))
        {
            return Err(AppError::parse(
                format!("items.{}", i.id),
                "invalid icon sha256",
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for b in &pack.banners {
            if !ids.insert(&b.id) {
                return Err(AppError::parse(
                    format!("banners.{}", b.id),
                    "duplicate banner id",
                ));
            }
        }
        Ok(pack)
    }

    /// 某语言的物品名；缺失时为 `None`。
    pub fn item_name(&self, id: &str, lang: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|i| i.id == id)
            .and_then(|i| i.names.get(lang))
            .map(String::as_str)
    }

    /// 按物品 ID 或任一语言名称查找物品。
    pub fn item(&self, key: &str) -> Option<&PackItem> {
        self.items.iter().find(|i| i.matches(key))
    }

    /// 头像索引：物品 ID 与各语言名称 → 图标哈希。记录里不论是 ID 还是哪种语言的名称都能查到。
    pub fn icon_index(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for item in &self.items {
            let Some(icon) = &item.icon else { continue };
            for key in std::iter::once(&item.id).chain(item.names.values()) {
                out.entry(key.clone()).or_insert_with(|| icon.clone());
            }
        }
        out
    }

    /// 名单展开：每个引用加上该物品的各语言名称。
    fn expand(&self, refs: &[String]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in refs {
            let item = self.item(r);
            let names = item
                .into_iter()
                .flat_map(|i| std::iter::once(&i.id).chain(i.names.values()));
            for n in std::iter::once(r).chain(names) {
                if !out.contains(n) {
                    out.push(n.clone());
                }
            }
        }
        out
    }

    /// 转成统计引擎用的 [`GameMeta`]。
    pub fn to_game_meta(&self) -> Result<GameMeta> {
        let mut banners = Vec::with_capacity(self.banners.len());
        for b in &self.banners {
            let loc = format!("banners.{}", b.id);
            let start = time_ms(&b.start, &loc)?;
            let end = time_ms(&b.end, &loc)?;
            if start >= end {
                return Err(AppError::parse(loc, "start must be before end"));
            }
            banners.push(Banner {
                id: b.id.clone(),
                pool_type: PoolTypeId::from(b.pool_type.as_str()),
                name: b
                    .names
                    .get("zh-cn")
                    .or_else(|| b.names.values().next())
                    .cloned()
                    .unwrap_or_else(|| b.id.clone()),
                start,
                end,
                up_top: self.expand(&b.up_top),
                up_second: self.expand(&b.up_second),
            });
        }
        Ok(GameMeta {
            game: Some(GameId::from(self.game.as_str())),
            banners,
            standard: self.expand(&self.standard),
        })
    }
}

/// 版本号按 `.` 分段、逐段按数字比较（非数字段视为 0）。
pub fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split(['.', '-'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (mut a, mut b) = (parse(a), parse(b));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a.cmp(&b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    const PACK: &str = r#"{
      "game": "genshin",
      "items": [
        {"id": "10000022", "rarity": 5, "names": {"zh-cn": "温迪", "en-us": "Venti"}},
        {"id": "10000016", "rarity": 5, "names": {"zh-cn": "迪卢克", "en-us": "Diluc"}}
      ],
      "standard": ["10000016"],
      "banners": [
        {"id": "b1", "pool_type": "character", "names": {"en-us": "Ballad in Goblets"},
         "start": "2020-09-28T06:00:00+08:00", "end": "2020-10-18T17:59:59+08:00",
         "up_top": ["10000022", "可莉"], "up_second": []}
      ]
    }"#;

    #[test]
    fn expands_names_and_times() {
        let pack = GamePack::parse(PACK.as_bytes()).unwrap();
        let meta = pack.to_game_meta().unwrap();
        assert_eq!(meta.game.as_ref().unwrap(), "genshin");
        assert_eq!(meta.standard, ["10000016", "Diluc", "迪卢克"]);
        let b = &meta.banners[0];
        assert_eq!(b.up_top, ["10000022", "Venti", "温迪", "可莉"]);
        assert_eq!(b.name, "Ballad in Goblets");
        assert_eq!(b.start, 1_601_244_000_000);
        assert_eq!(pack.item_name("10000022", "en-us"), Some("Venti"));
        assert_eq!(pack.item_name("10000022", "ja-jp"), None);
    }

    #[test]
    fn rejects_bad_packs() {
        let bad_time = PACK.replace("2020-09-28T06:00:00+08:00", "2020-09-28");
        assert_eq!(
            GamePack::parse(bad_time.as_bytes()).unwrap_err().code(),
            "parse"
        );
        let reversed = PACK.replace("2020-10-18T17:59:59+08:00", "2020-09-01T00:00:00+08:00");
        assert!(GamePack::parse(reversed.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("start must be before end"));
        let mut pack: GamePack = serde_json::from_str(PACK).unwrap();
        pack.banners.push(pack.banners[0].clone());
        let dup = serde_json::to_vec(&pack).unwrap();
        assert!(GamePack::parse(&dup)
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
        assert!(GamePack::parse(b"[]").is_err());
    }

    #[test]
    fn icon_index_and_lookup() {
        let sha = "ab".repeat(32);
        let with_icon = PACK.replace(
            r#""names": {"zh-cn": "温迪", "en-us": "Venti"}}"#,
            &format!(r#""names": {{"zh-cn": "温迪", "en-us": "Venti"}}, "icon": "{sha}"}}"#),
        );
        let pack = GamePack::parse(with_icon.as_bytes()).unwrap();
        let index = pack.icon_index();
        assert_eq!(index.len(), 3);
        assert_eq!(index["Venti"], sha);
        assert_eq!(index["10000022"], sha);
        assert!(!index.contains_key("迪卢克"));
        assert_eq!(pack.item("迪卢克").unwrap().id, "10000016");
        assert!(pack.item("可莉").is_none());
        // 序列化时没有图标的物品不写 icon 字段
        let out = serde_json::to_string(&pack).unwrap();
        assert_eq!(out.matches("icon").count(), 1);

        let bad = with_icon.replace(&sha, "ABC");
        assert!(GamePack::parse(bad.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("invalid icon"));
        assert!(!is_sha256(&"AB".repeat(32)));
        assert!(!is_sha256("../x"));
    }

    #[test]
    fn versions() {
        assert_eq!(cmp_version("2026.10.26", "2026.9.30"), Ordering::Greater);
        assert_eq!(cmp_version("2026.10.26", "2026.10.26.1"), Ordering::Less);
        assert_eq!(cmp_version("v0.1.0", "0.1"), Ordering::Equal);
        assert_eq!(cmp_version("0.2.0-beta", "0.2.0"), Ordering::Equal);
    }
}
