//! GachaNote 游戏插件：每款游戏由「规则模板 + 拉取器 + 卡池类型映射」组成，
//! 文件格式转换器在 `gachanote-formats` 中跨游戏通用。
//!
//! | 游戏 | 模块 | 在线拉取 |
//! | --- | --- | --- |
//! | 原神 | [`genshin`] | 游戏网页缓存中的链接（[`hoyo`]） |
//! | 崩坏：星穹铁道 | [`starrail`] | 同上 |
//! | 绝区零 | [`zzz`] | 同上 |
//! | 鸣潮 | [`wuwa`] | Client.log 中的链接 |
//! | 明日方舟 | [`arknights`] | 鹰角账号 token（v0.2） |
//! | 明日方舟：终末地 | [`endfield`] | 同上 |

pub mod arknights;
pub mod endfield;
pub mod genshin;
pub mod hoyo;
mod net;
mod paths;
mod rules;
pub mod starrail;
pub mod wuwa;
pub mod zzz;

use std::sync::Arc;

use gachanote_core::plugin::{GamePlugin, PluginRegistry};
use gachanote_core::rules::PoolRule;
use gachanote_core::{GameId, PoolTypeId};

pub use net::{default_client, Pacing};
pub use paths::StandardPaths;

use hoyo::{HoyoFetcher, HoyoGame, HoyoUrl};
use wuwa::{KuroFetcher, KuroUrl};

/// 规则模板中的卡池类型，按首次出现的顺序去重（即界面显示顺序）。
pub(crate) fn pool_order(rules: &[PoolRule]) -> Vec<PoolTypeId> {
    let mut out: Vec<PoolTypeId> = Vec::new();
    for r in rules {
        if !out.contains(&r.pool_type) {
            out.push(r.pool_type.clone());
        }
    }
    out
}

/// 首版六款游戏的插件，按侧边栏顺序。拉取器共用 `client`（代理设置）与请求节奏。
pub fn all_plugins(client: &reqwest::Client, pacing: &Pacing) -> Vec<Arc<dyn GamePlugin>> {
    let hoyo = |g| HoyoFetcher::with_client(g, client.clone(), pacing.clone());
    vec![
        Arc::new(genshin::Genshin::new(hoyo(HoyoGame::Genshin))),
        Arc::new(starrail::StarRail::new(hoyo(HoyoGame::StarRail))),
        Arc::new(zzz::Zzz::new(hoyo(HoyoGame::Zzz))),
        Arc::new(arknights::Arknights::default()),
        Arc::new(endfield::Endfield::default()),
        Arc::new(wuwa::Wuwa::new(KuroFetcher::new(
            client.clone(),
            pacing.clone(),
        ))),
    ]
}

/// 把六款游戏的插件与全部格式转换器注册到 `registry`。
pub fn register_all(registry: &mut PluginRegistry, client: &reqwest::Client, pacing: &Pacing) {
    for p in all_plugins(client, pacing) {
        registry.register_game(p);
    }
    for c in gachanote_formats::all_converters() {
        registry.register_converter(c);
    }
}

/// 识别用户粘贴的抽卡链接属于哪款游戏（FR-FET-03）。
pub fn detect_url(raw: &str) -> Option<GameId> {
    if let Some(u) = HoyoUrl::parse(raw) {
        return Some(u.game.id());
    }
    KuroUrl::parse(raw).map(|_| GameId::WUWA.into())
}

#[cfg(test)]
mod tests;
