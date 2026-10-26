//! 原神：规则模板（含命定值）、卡池类型映射与 URL 拉取（T-20）。

use gachanote_core::plugin::{FetchKind, Fetcher, GameInfo, GamePlugin, Vendor};
use gachanote_core::rules::{Mechanic, PoolRule, UpRule};
use gachanote_core::{FormatId, GameId, PoolTypeId};

use crate::hoyo::{HoyoFetcher, HoyoGame};
use crate::rules::{curve, digits, fifty_fifty, rule, RuleExt};

/// 2.0 版本（2021-07-21 06:00 UTC+8）：武器池加入神铸定轨，命定值上限 2。
pub const V2_0: i64 = 1_626_818_400_000;
/// 5.0 版本（2024-08-28 06:00 UTC+8）：命定值上限改为 1；角色池加入「捕获明光」。
pub const V5_0: i64 = 1_724_796_000_000;

/// 官方 `gacha_type` → 卡池类型。两个角色活动祈愿（301、400）共享保底。
pub fn pool_type(code: &str) -> Option<PoolTypeId> {
    let p = match code {
        "301" | "400" => "character",
        "302" => "weapon",
        "200" => "standard",
        "500" => "chronicled",
        "100" => "beginner",
        _ => return None,
    };
    Some(p.into())
}

/// 规则模板（SRS 第 5 节）。
pub fn rules() -> Vec<PoolRule> {
    let char_curve = curve(0.006, Some(74), 0.06, 90);
    let weapon_curve = curve(0.007, Some(63), 0.07, 80);
    vec![
        rule("character", 5, char_curve, fifty_fifty(0.5)),
        rule("character", 5, char_curve, fifty_fifty(0.5))
            .since(V5_0)
            .with(Mechanic::CapturingRadiance),
        rule("weapon", 5, weapon_curve, fifty_fifty(0.75)),
        rule("weapon", 5, weapon_curve, fifty_fifty(0.75))
            .since(V2_0)
            .with(Mechanic::EpitomizedPath { max: 2 }),
        rule("weapon", 5, weapon_curve, fifty_fifty(0.75))
            .since(V5_0)
            .with(Mechanic::EpitomizedPath { max: 1 }),
        rule("standard", 5, char_curve, UpRule::None),
        // 集录祈愿：定轨对象在记录里看不出来，只统计保底，不判断 UP
        rule("chronicled", 5, char_curve, UpRule::None),
        rule("beginner", 5, char_curve, UpRule::None),
    ]
}

/// 原神插件。
pub struct Genshin {
    rules: Vec<PoolRule>,
    fetcher: HoyoFetcher,
}

impl Genshin {
    /// 使用给定的拉取器。
    pub fn new(fetcher: HoyoFetcher) -> Self {
        Self {
            rules: rules(),
            fetcher,
        }
    }
}

impl Default for Genshin {
    fn default() -> Self {
        Self::new(HoyoFetcher::new(HoyoGame::Genshin))
    }
}

impl GamePlugin for Genshin {
    fn id(&self) -> GameId {
        GameId::GENSHIN.into()
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            id: self.id(),
            vendor: Vendor::Hoyoverse,
            theme_color: "#C8A464".into(),
            pool_types: crate::pool_order(&self.rules),
            fetch: Some(FetchKind::Url),
            formats: vec![
                FormatId::UigfV4,
                FormatId::UigfV3,
                FormatId::UigfV2,
                FormatId::GnCsv,
            ],
        }
    }

    fn rules(&self) -> &[PoolRule] {
        &self.rules
    }

    fn pool_type(&self, official: &str) -> Option<PoolTypeId> {
        pool_type(official)
    }

    /// 9 或 10 位数字。
    fn validate_uid(&self, uid: &str) -> bool {
        digits(uid, 9, 10)
    }

    fn fetcher(&self) -> Option<&dyn Fetcher> {
        Some(&self.fetcher)
    }
}
