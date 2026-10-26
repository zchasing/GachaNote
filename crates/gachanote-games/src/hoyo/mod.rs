//! 米哈游三款（原神、崩铁、绝区零）共用的部分：游戏参数、抽卡链接解析、
//! 磁盘缓存中的链接查找与分页拉取器（架构文档 7.2）。
//!
//! 接口行为参考 [HoYo.Gacha](https://github.com/lgou2w/HoYo.Gacha)（MIT / Apache-2.0）。

mod cache;
mod fetcher;
mod url;

pub use cache::{cache_urls, data_dirs};
pub use fetcher::HoyoFetcher;
pub use url::HoyoUrl;

use gachanote_core::{GameId, PoolTypeId};

use crate::{genshin, starrail, zzz};

/// 米哈游的一款游戏。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoyoGame {
    /// 原神（`hk4e`）。
    Genshin,
    /// 崩坏：星穹铁道（`hkrpg`）。
    StarRail,
    /// 绝区零（`nap`）。
    Zzz,
}

impl HoyoGame {
    /// 全部三款。
    pub const ALL: [Self; 3] = [Self::Genshin, Self::StarRail, Self::Zzz];

    /// 游戏 ID。
    pub fn id(self) -> GameId {
        match self {
            Self::Genshin => GameId::GENSHIN,
            Self::StarRail => GameId::STARRAIL,
            Self::Zzz => GameId::ZZZ,
        }
        .into()
    }

    /// `game_biz` 参数的前缀。
    pub fn biz(self) -> &'static str {
        match self {
            Self::Genshin => "hk4e",
            Self::StarRail => "hkrpg",
            Self::Zzz => "nap",
        }
    }

    /// 按 `game_biz` 参数（如 `hk4e_cn`）找游戏。
    pub fn from_biz(biz: &str) -> Option<Self> {
        let prefix = biz.split('_').next()?;
        Self::ALL.into_iter().find(|g| g.biz() == prefix)
    }

    /// 需要逐个拉取的卡池类型代码。常驻放在最前：它最可能有记录，用来校验链接。
    pub fn query_types(self) -> &'static [&'static str] {
        match self {
            Self::Genshin => &["200", "301", "302", "500", "100"],
            Self::StarRail => &["1", "11", "12", "21", "22", "2"],
            Self::Zzz => &["1", "2", "3", "5", "102", "103"],
        }
    }

    /// 请求中卡池类型参数的名称。
    fn type_param(self) -> &'static str {
        match self {
            Self::Zzz => "real_gacha_type",
            _ => "gacha_type",
        }
    }

    /// 记录中卡池 ID 字段（原神没有）。
    pub(crate) fn banner_key(self) -> Option<&'static str> {
        match self {
            Self::Genshin => None,
            _ => Some("gacha_id"),
        }
    }

    /// 游戏数据目录名（`<安装目录>/<名称>/webCaches`）。
    pub fn data_folder_names(self) -> &'static [&'static str] {
        match self {
            Self::Genshin => &["YuanShen_Data", "GenshinImpact_Data"],
            Self::StarRail => &["StarRail_Data"],
            Self::Zzz => &["ZenlessZoneZero_Data"],
        }
    }

    /// 官方卡池类型代码 → 本软件的卡池类型。
    pub fn pool_type(self, code: &str) -> Option<PoolTypeId> {
        match self {
            Self::Genshin => genshin::pool_type(code),
            Self::StarRail => starrail::pool_type(code),
            Self::Zzz => zzz::pool_type(code),
        }
    }

    /// 抽卡记录接口地址。崩铁联动跃迁走单独的接口。
    pub fn endpoint(self, global: bool, gacha_type: &str) -> &'static str {
        match (self, global) {
            (Self::Genshin, false) => {
                "https://public-operation-hk4e.mihoyo.com/gacha_info/api/getGachaLog"
            }
            (Self::Genshin, true) => {
                "https://public-operation-hk4e-sg.hoyoverse.com/gacha_info/api/getGachaLog"
            }
            (Self::StarRail, global) => {
                let collab = matches!(gacha_type, "21" | "22");
                match (global, collab) {
                    (false, false) => "https://public-operation-hkrpg.mihoyo.com/common/hkrpg_gacha_record/api/getGachaLog",
                    (false, true) => "https://public-operation-hkrpg.mihoyo.com/common/hkrpg_gacha_record/api/getLdGachaLog",
                    (true, false) => "https://public-operation-hkrpg-sg.hoyoverse.com/common/hkrpg_gacha_record/api/getGachaLog",
                    (true, true) => "https://public-operation-hkrpg-sg.hoyoverse.com/common/hkrpg_gacha_record/api/getLdGachaLog",
                }
            }
            (Self::Zzz, false) => {
                "https://public-operation-nap.mihoyo.com/common/gacha_record/api/getGachaLog"
            }
            (Self::Zzz, true) => {
                "https://public-operation-nap-sg.hoyoverse.com/common/gacha_record/api/getGachaLog"
            }
        }
    }
}

/// 按服务器代码（链接的 `region` 参数）取时区偏移（分钟）：美服 UTC-5，欧服 UTC+1，其余 UTC+8。
pub fn tz_of_region(region: &str) -> i32 {
    let r = region.to_ascii_lowercase();
    if r.ends_with("usa") || r.ends_with("_us") {
        -300
    } else if r.ends_with("euro") || r.ends_with("eur") || r.ends_with("_eu") {
        60
    } else {
        480
    }
}

/// 按 UID 推断时区（分钟）。9 位 UID 看第 1 位、10 位看第 2 位（规则同 HoYo.Gacha）；
/// 绝区零 10 位 UID 的区服数字含义不同。
pub fn tz_of_uid(game: HoyoGame, uid: &str) -> i32 {
    let digit = match uid.len() {
        9 => uid.chars().next(),
        10 => uid.chars().nth(1),
        _ => None,
    }
    .and_then(|c| c.to_digit(10));
    match (game, uid.len(), digit) {
        (HoyoGame::Zzz, 10, Some(0)) => -300,
        (HoyoGame::Zzz, 10, Some(5)) => 60,
        (HoyoGame::Zzz, _, _) => 480,
        (_, _, Some(6)) => -300,
        (_, _, Some(7)) => 60,
        _ => 480,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_params() {
        assert_eq!(HoyoGame::from_biz("hk4e_cn"), Some(HoyoGame::Genshin));
        assert_eq!(HoyoGame::from_biz("hkrpg_global"), Some(HoyoGame::StarRail));
        assert_eq!(HoyoGame::from_biz("nap_cn"), Some(HoyoGame::Zzz));
        assert_eq!(HoyoGame::from_biz("bh3_cn"), None);
        for g in HoyoGame::ALL {
            assert_eq!(HoyoGame::from_biz(g.biz()), Some(g));
            // 每个要拉取的卡池类型都能映射
            for t in g.query_types() {
                assert!(g.pool_type(t).is_some(), "{g:?} {t}");
            }
            assert!(!g.data_folder_names().is_empty());
        }
        assert_eq!(HoyoGame::Zzz.type_param(), "real_gacha_type");
        assert_eq!(HoyoGame::Genshin.banner_key(), None);
        assert!(HoyoGame::StarRail
            .endpoint(false, "21")
            .ends_with("getLdGachaLog"));
        assert!(HoyoGame::StarRail
            .endpoint(true, "11")
            .contains("hkrpg-sg.hoyoverse.com"));
        assert!(HoyoGame::Genshin.endpoint(true, "301").contains("hk4e-sg"));
        assert!(HoyoGame::Zzz.endpoint(false, "2").contains("nap.mihoyo"));
    }

    #[test]
    fn timezones() {
        assert_eq!(tz_of_region("cn_gf01"), 480);
        assert_eq!(tz_of_region("os_usa"), -300);
        assert_eq!(tz_of_region("prod_official_usa"), -300);
        assert_eq!(tz_of_region("prod_gf_us"), -300);
        assert_eq!(tz_of_region("os_euro"), 60);
        assert_eq!(tz_of_region("prod_official_eur"), 60);
        assert_eq!(tz_of_region("prod_gf_eu"), 60);
        assert_eq!(tz_of_region("os_asia"), 480);
        assert_eq!(tz_of_uid(HoyoGame::Genshin, "600000001"), -300);
        assert_eq!(tz_of_uid(HoyoGame::Genshin, "1700000001"), 60);
        assert_eq!(tz_of_uid(HoyoGame::StarRail, "800000001"), 480);
        assert_eq!(tz_of_uid(HoyoGame::Zzz, "1000000001"), -300);
        assert_eq!(tz_of_uid(HoyoGame::Zzz, "1500000001"), 60);
        assert_eq!(tz_of_uid(HoyoGame::Zzz, "10000001"), 480);
        assert_eq!(tz_of_uid(HoyoGame::Genshin, "x"), 480);
    }
}
