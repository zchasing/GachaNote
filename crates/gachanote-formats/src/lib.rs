//! GachaNote 格式转换：UIGF v2–v4.2、SRGF、GNJ 与 CSV 的编解码。
//!
//! | 格式 | 游戏 | 导入 | 导出 |
//! | --- | --- | --- | --- |
//! | UIGF v4.x | 原神、崩铁、绝区零 | ✓ | ✓（v4.2） |
//! | UIGF v2.x / v3.0 | 原神 | ✓ | — |
//! | SRGF v1.0 | 崩铁 | ✓ | — |
//! | GNJ | 全部（主要用于鹰角两款与鸣潮） | ✓ | ✓ |
//! | arknights-gacha-export CSV | 明日方舟、终末地 | ✓ | — |
//! | 自有 CSV | 全部 | ✓ | ✓ |
//!
//! 解码只做与格式有关的换算（时区换算成 UTC、方舟星级 0–5 → 1–6），卡池类型映射由游戏插件完成。

mod csv_formats;
pub mod gnj;
pub mod uigf;
mod util;

use std::sync::Arc;

use gachanote_core::plugin::Converter;

pub use csv_formats::{AkExportCsv, GnCsv, AK_HEADER, EF_HEADER, GN_HEADER};
pub use gnj::Gnj;
pub use uigf::{Srgf, UigfV2, UigfV3, UigfV4};
pub use util::{local_to_utc_ms, utc_ms_to_local};

/// 全部转换器，按嗅探优先级排列。
pub fn all_converters() -> Vec<Arc<dyn Converter>> {
    vec![
        Arc::new(UigfV4),
        Arc::new(UigfV3),
        Arc::new(UigfV2),
        Arc::new(Srgf),
        Arc::new(Gnj),
        Arc::new(AkExportCsv),
        Arc::new(GnCsv),
    ]
}
