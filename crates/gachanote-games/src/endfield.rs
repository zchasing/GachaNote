//! 明日方舟：终末地：规则模板与卡池类型映射（T-24）。token 拉取见 [`crate::hypergryph`]。
//!
//! 记录的官方卡池代码由转换器写成 `<char|weapon>:<poolId>`。免费抽与赠送条目在解码时已标为
//! `is_pull = false`，不计抽数与保底（[`Mechanic::FreePulls`]）。

use gachanote_core::plugin::{FetchKind, Fetcher, GameInfo, GamePlugin, Vendor};
use gachanote_core::rules::{Mechanic, PoolRule, UpRule};
use gachanote_core::{FormatId, GameId, PoolTypeId};

use crate::hypergryph::HgFetcher;
use crate::rules::{curve, digits, rule, RuleExt};

/// 官方卡池代码 → 卡池类型。
pub fn pool_type(code: &str) -> Option<PoolTypeId> {
    let (kind, pool_id) = code.split_once(':')?;
    let pool_id = pool_id.to_ascii_lowercase();
    let p = match kind {
        "weapon" => "weapon",
        "char" if pool_id.starts_with("special") => "character",
        "char" if pool_id.starts_with("standard") => "standard",
        "char" if pool_id.starts_with("beginner") => "beginner",
        "char" => "other",
        _ => return None,
    };
    Some(p.into())
}

/// 规则模板（SRS 第 5 节）。
pub fn rules() -> Vec<PoolRule> {
    let char_curve = curve(0.008, Some(65), 0.05, 80);
    vec![
        // 角色限定池：50% UP，没有大保底；当期 120 抽必得 UP（换期清零），80 抽保底跨期继承
        rule("character", 6, char_curve, UpRule::Fixed { rate: 0.5 })
            .with(Mechanic::UpWithinN {
                n: 120,
                per_banner: true,
            })
            .with(Mechanic::FreePulls),
        // 武器池：4%，40 抽必出；当期 80 抽内必得 UP 武器。UP 占比 25% 待确认
        rule(
            "weapon",
            6,
            curve(0.04, None, 0.0, 40),
            UpRule::Fixed { rate: 0.25 },
        )
        .with(Mechanic::UpWithinN {
            n: 80,
            per_banner: true,
        }),
        rule("standard", 6, char_curve, UpRule::None),
        rule("beginner", 6, char_curve, UpRule::None),
        rule("other", 6, char_curve, UpRule::None),
    ]
}

/// 终末地插件。
pub struct Endfield {
    rules: Vec<PoolRule>,
    fetcher: Option<HgFetcher>,
}

impl Default for Endfield {
    /// 没有拉取器（只能导入文件）。
    fn default() -> Self {
        Self {
            rules: rules(),
            fetcher: None,
        }
    }
}

impl Endfield {
    /// 带鹰角 token 拉取器。
    pub fn new(fetcher: HgFetcher) -> Self {
        Self {
            rules: rules(),
            fetcher: Some(fetcher),
        }
    }
}

impl GamePlugin for Endfield {
    fn id(&self) -> GameId {
        GameId::ENDFIELD.into()
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            id: self.id(),
            vendor: Vendor::Hypergryph,
            theme_color: "#F2C200".into(),
            pool_types: crate::pool_order(&self.rules),
            fetch: Some(FetchKind::AccountToken),
            formats: vec![FormatId::Gnj, FormatId::AkExportCsv, FormatId::GnCsv],
        }
    }

    fn rules(&self) -> &[PoolRule] {
        &self.rules
    }

    fn pool_type(&self, official: &str) -> Option<PoolTypeId> {
        pool_type(official)
    }

    fn fetcher(&self) -> Option<&dyn Fetcher> {
        self.fetcher.as_ref().map(|f| f as &dyn Fetcher)
    }

    /// 1–16 位数字。
    fn validate_uid(&self, uid: &str) -> bool {
        digits(uid, 1, 16)
    }
}
