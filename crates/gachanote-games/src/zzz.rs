//! 绝区零：规则模板、卡池类型映射与 URL 拉取（T-22）。
//!
//! 星级按记录原文：S 级 = 4、A 级 = 3、B 级 = 2，所以最高星为 4。

use gachanote_core::plugin::{FetchKind, Fetcher, GameInfo, GamePlugin, Vendor};
use gachanote_core::rules::{Mechanic, PoolRule, UpRule};
use gachanote_core::{FormatId, GameId, PoolTypeId};

use crate::hoyo::{HoyoFetcher, HoyoGame};
use crate::rules::{curve, digits, fifty_fifty, rule, RuleExt};

/// 官方卡池类型 → 卡池类型。接口按 `real_gacha_type`（1/2/3/5/102/103）查询，
/// 记录里的 `gacha_type` 可能是四位数（如 `2001`），取首位归类。
pub fn pool_type(code: &str) -> Option<PoolTypeId> {
    let base = match code {
        "102" | "103" => code,
        c if c.len() == 4 && c.bytes().all(|b| b.is_ascii_digit()) => &c[..1],
        c => c,
    };
    let p = match base {
        "2" => "character",
        "3" => "w_engine",
        "1" => "standard",
        "5" => "bangboo",
        "102" => "exclusive_rescreening",
        "103" => "w_engine_reverberation",
        _ => return None,
    };
    Some(p.into())
}

/// 规则模板（SRS 第 5 节）。独家重映、音擎回响可自选目标，不判断 UP。
pub fn rules() -> Vec<PoolRule> {
    let agent_curve = curve(0.006, Some(75), 0.06, 90);
    let engine_curve = curve(0.01, Some(65), 0.07, 80);
    vec![
        rule("character", 4, agent_curve, fifty_fifty(0.5)),
        rule("w_engine", 4, engine_curve, fifty_fifty(0.75)),
        rule("standard", 4, agent_curve, UpRule::None).with(Mechanic::Spark { n: 300 }),
        rule("bangboo", 4, engine_curve, UpRule::None),
        rule("exclusive_rescreening", 4, agent_curve, UpRule::None),
        rule("w_engine_reverberation", 4, engine_curve, UpRule::None),
    ]
}

/// 绝区零插件。
pub struct Zzz {
    rules: Vec<PoolRule>,
    fetcher: HoyoFetcher,
}

impl Zzz {
    /// 使用给定的拉取器。
    pub fn new(fetcher: HoyoFetcher) -> Self {
        Self {
            rules: rules(),
            fetcher,
        }
    }
}

impl Default for Zzz {
    fn default() -> Self {
        Self::new(HoyoFetcher::new(HoyoGame::Zzz))
    }
}

impl GamePlugin for Zzz {
    fn id(&self) -> GameId {
        GameId::ZZZ.into()
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            id: self.id(),
            vendor: Vendor::Hoyoverse,
            theme_color: "#B5D800".into(),
            pool_types: crate::pool_order(&self.rules),
            fetch: Some(FetchKind::Url),
            formats: vec![FormatId::UigfV4, FormatId::GnCsv],
        }
    }

    fn rules(&self) -> &[PoolRule] {
        &self.rules
    }

    fn pool_type(&self, official: &str) -> Option<PoolTypeId> {
        pool_type(official)
    }

    /// 国服 8 位、国际服 10 位数字。
    fn validate_uid(&self, uid: &str) -> bool {
        digits(uid, 8, 10)
    }

    fn fetcher(&self) -> Option<&dyn Fetcher> {
        Some(&self.fetcher)
    }
}
