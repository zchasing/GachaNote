//! 明日方舟：规则模板与卡池类型映射（T-24）。token 拉取在 v0.2（T-51）。
//!
//! 记录自带卡池 ID（`poolId`，如 `LIMITED_37_0_1`），按前缀归入卡池类型；
//! 没有大小保底，歪率口径为「非 UP 六星占比」（[`UpRule::Fixed`]）。

use gachanote_core::plugin::{GameInfo, GamePlugin, Vendor};
use gachanote_core::rules::{Mechanic, PoolRule, UpRule};
use gachanote_core::{FormatId, GameId, PoolTypeId};

use crate::rules::{curve, digits, rule, RuleExt};

/// 卡池 ID 前缀 → 卡池类型。不认识的前缀（`ATTAIN`、`SPECIAL` 定向甄选等）归入 `other`
/// （每期单独计保底、不判断 UP），
/// 保证记录不丢，等确认规则后再细分。
pub fn pool_type(code: &str) -> Option<PoolTypeId> {
    let prefix = code.split('_').next()?.to_ascii_uppercase();
    let p = match prefix.as_str() {
        "" => return None,
        // 标准寻访；`DOUBLE` 是双六星 UP 的新版标准寻访，保底与 `NORM` 共享
        "NORM" | "DOUBLE" => "standard",
        "CLASSIC" | "FESCLASSIC" => "classic",
        "LIMITED" => "limited",
        "SINGLE" => "single",
        "LINKAGE" => "linkage",
        "BOOT" => "beginner",
        _ => "other",
    };
    Some(p.into())
}

/// 规则模板（SRS 第 5 节）：基础 2%，50 抽后每抽 +2%，第 99 抽必出（推算）。
pub fn rules() -> Vec<PoolRule> {
    let c = curve(0.02, Some(51), 0.02, 99);
    vec![
        rule("standard", 6, c, UpRule::Fixed { rate: 0.5 }),
        rule("classic", 6, c, UpRule::Fixed { rate: 0.5 }),
        // 季节限定：两个 UP 合计 70%，300 抽可兑换，保底不继承
        rule("limited", 6, c, UpRule::Fixed { rate: 0.7 })
            .with(Mechanic::Spark { n: 300 })
            .per_banner(),
        // 登场限定（单 UP）：150 抽未出 UP 则下一只六星必为 UP，一次性
        rule("single", 6, c, UpRule::Fixed { rate: 0.5 })
            .with(Mechanic::UpWithinN {
                n: 150,
                per_banner: false,
            })
            .per_banner(),
        // 联动寻访：120 抽内必得联动六星
        rule("linkage", 6, c, UpRule::Fixed { rate: 0.5 })
            .with(Mechanic::UpWithinN {
                n: 120,
                per_banner: true,
            })
            .per_banner(),
        rule("beginner", 6, c, UpRule::None).per_banner(),
        rule("other", 6, c, UpRule::None).per_banner(),
    ]
}

/// 明日方舟插件。
pub struct Arknights {
    rules: Vec<PoolRule>,
}

impl Default for Arknights {
    fn default() -> Self {
        Self { rules: rules() }
    }
}

impl GamePlugin for Arknights {
    fn id(&self) -> GameId {
        GameId::ARKNIGHTS.into()
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            id: self.id(),
            vendor: Vendor::Hypergryph,
            theme_color: "#2F8FE0".into(),
            pool_types: crate::pool_order(&self.rules),
            fetch: None,
            formats: vec![FormatId::Gnj, FormatId::AkExportCsv, FormatId::GnCsv],
        }
    }

    fn rules(&self) -> &[PoolRule] {
        &self.rules
    }

    fn pool_type(&self, official: &str) -> Option<PoolTypeId> {
        pool_type(official)
    }

    /// 1–16 位数字。
    fn validate_uid(&self, uid: &str) -> bool {
        digits(uid, 1, 16)
    }
}
