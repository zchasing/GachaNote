//! 抽卡链接解析：从游戏内网页的地址或接口地址中取出 `authkey` 等参数。
//!
//! 缓存中的链接多为网页地址（`webstatic.mihoyo.com/.../index.html?...#/log`），
//! 也可能是接口地址；两者的查询参数相同，所以只取参数，再按游戏和服务器拼出接口地址。

use url::Url;

use super::{tz_of_region, HoyoGame};

/// 每次请求都重新设置、不沿用原链接的参数。
const VOLATILE: [&str; 5] = ["gacha_type", "real_gacha_type", "page", "size", "end_id"];

/// 解析后的米哈游抽卡链接。
#[derive(Clone, PartialEq, Eq)]
pub struct HoyoUrl {
    /// 游戏。
    pub game: HoyoGame,
    /// 是否国际服。
    pub global: bool,
    /// 服务器代码（`region` 参数），可能为空。
    pub region: String,
    /// 其余查询参数（含 authkey），按原顺序。
    params: Vec<(String, String)>,
}

impl std::fmt::Debug for HoyoUrl {
    // 参数里有 authkey，不能输出
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoyoUrl")
            .field("game", &self.game)
            .field("global", &self.global)
            .field("region", &self.region)
            .finish_non_exhaustive()
    }
}

impl HoyoUrl {
    /// 解析链接；不是带 `authkey` 的米哈游链接时返回 `None`。
    pub fn parse(raw: &str) -> Option<Self> {
        let url = Url::parse(raw.trim()).ok()?;
        let host = url.host_str()?.to_ascii_lowercase();
        let official = host.ends_with("mihoyo.com");
        if !official && !host.ends_with("hoyoverse.com") {
            return None;
        }
        // 有的链接把参数放在片段里（`#/log?...`）
        let query = match url.query() {
            Some(q) if !q.is_empty() => q.to_owned(),
            _ => url.fragment()?.split_once('?')?.1.to_owned(),
        };
        let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect();
        let get = |k: &str| {
            params
                .iter()
                .find(|(key, v)| key == k && !v.is_empty())
                .map(|(_, v)| v.as_str())
        };
        get("authkey")?;
        let biz = get("game_biz");
        let game = biz.and_then(HoyoGame::from_biz).or_else(|| {
            HoyoGame::ALL
                .into_iter()
                .find(|g| host.contains(g.biz()) || url.path().contains(g.biz()))
        })?;
        let global = biz.map_or(!official, |b| b.ends_with("_global"));
        let region = get("region").unwrap_or_default().to_owned();
        let params = params
            .into_iter()
            .filter(|(k, _)| !VOLATILE.contains(&k.as_str()))
            .collect();
        Some(Self {
            game,
            global,
            region,
            params,
        })
    }

    /// 时区偏移（分钟）；没有 `region` 参数时为 `None`，由调用方按 UID 推断。
    pub fn tz_offset_min(&self) -> Option<i32> {
        (!self.region.is_empty()).then(|| tz_of_region(&self.region))
    }

    /// 某个卡池类型、某一页的查询参数。
    pub(crate) fn query(&self, gacha_type: &str, end_id: &str, size: u32) -> Vec<(String, String)> {
        let mut q = self.params.clone();
        q.push((self.game.type_param().to_owned(), gacha_type.to_owned()));
        q.push(("size".into(), size.to_string()));
        q.push(("end_id".into(), end_id.to_owned()));
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENSHIN: &str = "https://webstatic.mihoyo.com/hk4e/event/e20190909gacha-v3/index.html?win_mode=fullscreen&authkey_ver=1&sign_type=2&auth_appid=webview_gacha&init_type=301&lang=zh-cn&region=cn_gf01&authkey=a%2Bb%3D&game_biz=hk4e_cn&gacha_type=301&page=1#/log";

    #[test]
    fn parses_webview_url() {
        let u = HoyoUrl::parse(GENSHIN).unwrap();
        assert_eq!(
            (u.game, u.global, u.region.as_str()),
            (HoyoGame::Genshin, false, "cn_gf01")
        );
        assert_eq!(u.tz_offset_min(), Some(480));
        let q = u.query("200", "0", 20);
        assert!(q.contains(&("authkey".into(), "a+b=".into())));
        assert!(q.contains(&("gacha_type".into(), "200".into())));
        assert!(!q.iter().any(|(k, _)| k == "page"));
        assert_eq!(q.iter().filter(|(k, _)| k == "gacha_type").count(), 1);
        // Debug 不含 authkey
        assert!(!format!("{u:?}").contains("a+b"));
    }

    #[test]
    fn parses_api_and_fragment_urls() {
        let u = HoyoUrl::parse("https://public-operation-hkrpg-sg.hoyoverse.com/common/hkrpg_gacha_record/api/getGachaLog?authkey=k&game_biz=hkrpg_global&region=prod_official_usa").unwrap();
        assert_eq!((u.game, u.global), (HoyoGame::StarRail, true));
        assert_eq!(u.tz_offset_min(), Some(-300));

        let u = HoyoUrl::parse(
            "https://webstatic.mihoyo.com/nap/event/e20230424gacha/index.html#/info?authkey=k&real_gacha_type=2",
        )
        .unwrap();
        assert_eq!((u.game, u.global), (HoyoGame::Zzz, false));
        assert_eq!(u.tz_offset_min(), None);
        let q = u.query("3", "9", 5);
        assert!(q.contains(&("real_gacha_type".into(), "3".into())));
        assert!(!q.contains(&("real_gacha_type".into(), "2".into())));
    }

    #[test]
    fn rejects_other_urls() {
        for s in [
            "not a url",
            "https://example.com/?authkey=1&game_biz=hk4e_cn",
            "https://webstatic.mihoyo.com/hk4e/index.html?game_biz=hk4e_cn",
            "https://webstatic.mihoyo.com/hk4e/index.html?authkey=",
            "https://webstatic.mihoyo.com/bh3/index.html?authkey=1",
            "https://webstatic.mihoyo.com/index.html#/log",
        ] {
            assert!(HoyoUrl::parse(s).is_none(), "{s}");
        }
    }
}
