//! 崩坏：星穹铁道：规则模板、卡池类型映射与 URL 拉取（T-22）。

use gachanote_core::plugin::{FetchKind, Fetcher, GameInfo, GamePlugin, Vendor};
use gachanote_core::rules::{Mechanic, PoolRule, UpRule};
use gachanote_core::{FormatId, GameId, PoolTypeId};

use crate::hoyo::{HoyoFetcher, HoyoGame};
use crate::rules::{curve, digits, fifty_fifty, rule, RuleExt};

/// 官方 `gacha_type` → 卡池类型。联动跃迁（21、22）单独计保底。
pub fn pool_type(code: &str) -> Option<PoolTypeId> {
    let p = match code {
        "11" => "character",
        "12" => "light_cone",
        "21" => "collab_character",
        "22" => "collab_light_cone",
        "1" => "standard",
        "2" => "beginner",
        _ => return None,
    };
    Some(p.into())
}

/// 规则模板（SRS 第 5 节）。
pub fn rules() -> Vec<PoolRule> {
    let char_curve = curve(0.006, Some(74), 0.06, 90);
    let cone_curve = curve(0.008, Some(66), 0.07, 80);
    vec![
        rule("character", 5, char_curve, fifty_fifty(0.5)),
        rule("light_cone", 5, cone_curve, fifty_fifty(0.75)),
        rule("collab_character", 5, char_curve, fifty_fifty(0.5)),
        rule("collab_light_cone", 5, cone_curve, fifty_fifty(0.75)),
        rule("standard", 5, char_curve, UpRule::None).with(Mechanic::Spark { n: 300 }),
        rule("beginner", 5, curve(0.006, None, 0.0, 50), UpRule::None),
    ]
}

/// 崩铁插件。
pub struct StarRail {
    rules: Vec<PoolRule>,
    fetcher: HoyoFetcher,
}

impl StarRail {
    /// 使用给定的拉取器。
    pub fn new(fetcher: HoyoFetcher) -> Self {
        Self {
            rules: rules(),
            fetcher,
        }
    }
}

impl Default for StarRail {
    fn default() -> Self {
        Self::new(HoyoFetcher::new(HoyoGame::StarRail))
    }
}

impl GamePlugin for StarRail {
    fn id(&self) -> GameId {
        GameId::STARRAIL.into()
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            id: self.id(),
            vendor: Vendor::Hoyoverse,
            theme_color: "#6F6BD9".into(),
            pool_types: crate::pool_order(&self.rules),
            fetch: Some(FetchKind::Url),
            formats: vec![FormatId::UigfV4, FormatId::SrgfV1, FormatId::GnCsv],
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
