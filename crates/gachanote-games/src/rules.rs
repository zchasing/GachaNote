//! 写规则模板用的小工具。参数取值见 SRS 第 5 节；软保底起点与递增幅度官方未公示，取社区统计值。

use gachanote_core::rules::{Mechanic, PityCurve, PityScope, PoolRule, UpRule};

/// 单抽概率曲线。
pub(crate) const fn curve(base: f64, soft_start: Option<u32>, step: f64, hard: u32) -> PityCurve {
    PityCurve {
        base,
        soft_start,
        step,
        hard,
    }
}

/// 一份一直有效、跨期继承保底、没有特殊机制的规则。
pub(crate) fn rule(pool_type: &str, rarity_top: u8, curve: PityCurve, up: UpRule) -> PoolRule {
    PoolRule {
        pool_type: pool_type.into(),
        since: None,
        rarity_top,
        curve,
        up,
        extras: Vec::new(),
        pity_scope: PityScope::SharedByType,
        achievements: Default::default(),
    }
}

/// 链式修改规则。
pub(crate) trait RuleExt {
    /// 从某时间（UTC 毫秒）起生效。
    fn since(self, ms: i64) -> Self;
    /// 叠加一个特殊机制。
    fn with(self, mechanic: Mechanic) -> Self;
    /// 每期卡池重置保底。
    fn per_banner(self) -> Self;
}

impl RuleExt for PoolRule {
    fn since(mut self, ms: i64) -> Self {
        self.since = Some(ms);
        self
    }

    fn with(mut self, mechanic: Mechanic) -> Self {
        self.extras.push(mechanic);
        self
    }

    fn per_banner(mut self) -> Self {
        self.pity_scope = PityScope::PerBanner;
        self
    }
}

/// 大小保底：`rate` 概率为 UP，歪了之后下一只必为 UP。
pub(crate) const fn fifty_fifty(rate: f64) -> UpRule {
    UpRule::FiftyFifty {
        rate,
        guarantee_next: true,
    }
}

/// UID 是否为 `min..=max` 位数字。
pub(crate) fn digits(uid: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&uid.len()) && uid.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders() {
        let r = rule(
            "weapon",
            5,
            curve(0.007, Some(63), 0.07, 80),
            fifty_fifty(0.75),
        )
        .since(10)
        .with(Mechanic::EpitomizedPath { max: 2 })
        .per_banner();
        assert_eq!(r.since, Some(10));
        assert_eq!(r.path_max(), Some(2));
        assert_eq!(r.pity_scope, PityScope::PerBanner);
        assert!(digits("123456789", 9, 10));
        assert!(!digits("12345678", 9, 10));
        assert!(!digits("12345678a", 9, 10));
    }
}
