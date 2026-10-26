//! 各类标识符。
//!
//! 游戏、卡池类型、格式都用字符串新类型而不是枚举：新增一款游戏只需新增插件，
//! 不必修改 `core`（SRS NFR-13）。

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// 从字符串构造。
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// 字符串形式。
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    };
}

string_id!(
    /// 游戏标识，如 `genshin`、`starrail`。首版六款见 [`GameId::GENSHIN`] 等常量。
    GameId
);
string_id!(
    /// 规范化后的卡池类型，同时也是保底共享组：共享保底计数的官方卡池类型
    /// （如原神 301 与 400）映射到同一个值。
    PoolTypeId
);
string_id!(
    /// 账号主键（UUID v7）。
    AccountId
);
string_id!(
    /// 抽卡记录主键（UUID v7）。
    RecordId
);
string_id!(
    /// 导入批次主键（UUID v7）。
    BatchId
);

impl GameId {
    /// 原神。
    pub const GENSHIN: &'static str = "genshin";
    /// 崩坏：星穹铁道。
    pub const STARRAIL: &'static str = "starrail";
    /// 绝区零。
    pub const ZZZ: &'static str = "zzz";
    /// 明日方舟。
    pub const ARKNIGHTS: &'static str = "arknights";
    /// 明日方舟：终末地。
    pub const ENDFIELD: &'static str = "endfield";
    /// 鸣潮。
    pub const WUWA: &'static str = "wuwa";
}

/// 文件格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatId {
    /// UIGF v4.x（v4.0–v4.2），一个文件可含原神、崩铁、绝区零。
    UigfV4,
    /// UIGF v3.0（仅原神）。
    UigfV3,
    /// UIGF v2.x（仅原神）。
    UigfV2,
    /// SRGF v1.0（仅崩铁）。
    SrgfV1,
    /// GachaNote JSON：鹰角两款与鸣潮的完整备份格式。
    Gnj,
    /// arknights-gacha-export 导出的 CSV（明日方舟、终末地）。
    AkExportCsv,
    /// 本软件导出的 CSV。
    GnCsv,
}

impl FormatId {
    /// 存进数据库与日志用的字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UigfV4 => "uigf_v4",
            Self::UigfV3 => "uigf_v3",
            Self::UigfV2 => "uigf_v2",
            Self::SrgfV1 => "srgf_v1",
            Self::Gnj => "gnj",
            Self::AkExportCsv => "ak_export_csv",
            Self::GnCsv => "gn_csv",
        }
    }
}

impl fmt::Display for FormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_id_conversions() {
        let game = GameId::from(GameId::GENSHIN);
        assert_eq!(game, "genshin");
        assert_eq!(game.as_str(), "genshin");
        assert_eq!(game.to_string(), "genshin");
        assert_eq!(
            GameId::new(String::from("wuwa")),
            GameId::from("wuwa".to_owned())
        );
        let s: &str = game.as_ref();
        assert_eq!(s, "genshin");
        let mut set = std::collections::HashSet::new();
        set.insert(PoolTypeId::from("character"));
        assert!(set.contains("character"));
        assert_eq!(serde_json::to_string(&game).unwrap(), "\"genshin\"");
    }

    #[test]
    fn format_id_strings() {
        for (f, s) in [
            (FormatId::UigfV4, "uigf_v4"),
            (FormatId::UigfV3, "uigf_v3"),
            (FormatId::UigfV2, "uigf_v2"),
            (FormatId::SrgfV1, "srgf_v1"),
            (FormatId::Gnj, "gnj"),
            (FormatId::AkExportCsv, "ak_export_csv"),
            (FormatId::GnCsv, "gn_csv"),
        ] {
            assert_eq!(f.as_str(), s);
            assert_eq!(f.to_string(), s);
            assert_eq!(serde_json::to_string(&f).unwrap(), format!("\"{s}\""));
        }
    }
}
