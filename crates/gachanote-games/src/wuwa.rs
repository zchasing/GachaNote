//! 鸣潮：规则模板、卡池类型映射，以及从 Client.log 取链接的拉取器（T-23）。
//!
//! 抽卡链接写在游戏日志 `Client/Saved/Logs/Client.log` 里，形如
//! `https://aki-gm-resources[-oversea].aki-game.(com|net)/aki/gacha/index.html#/record?svr_id=...&player_id=...&record_id=...`。
//! 接口按卡池类型 POST 查询，一次返回该类型全部记录（官方只保留 6 个月），没有记录 ID，按自然键去重。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use async_trait::async_trait;
use gachanote_core::exchange::{ImportedAccount, ImportedRecord, PoolRef, RecordFailure};
use gachanote_core::plugin::{
    FetchKind, FetchSource, Fetcher, GameInfo, GamePlugin, PathProvider, SinceLookup, Vendor,
};
use gachanote_core::rules::{PoolRule, UpRule};
use gachanote_core::task::{CancelToken, Progress, ProgressEvent, Stage};
use gachanote_core::{AppError, FormatId, GameId, PoolTypeId, Redacted, Result};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::net::{check_status, default_client, network_error, with_retry, Pacing};
use crate::rules::{curve, digits, fifty_fifty, rule};

/// 需要逐个拉取的卡池类型。
pub const QUERY_TYPES: [u32; 7] = [1, 2, 3, 4, 5, 6, 7];

/// 官方 `cardPoolType` → 卡池类型。
pub fn pool_type(code: &str) -> Option<PoolTypeId> {
    let p = match code {
        "1" => "character",
        "2" => "weapon",
        "3" => "standard_character",
        "4" => "standard_weapon",
        "5" => "beginner",
        "6" => "beginner_selector",
        "7" => "giveback_selector",
        _ => return None,
    };
    Some(p.into())
}

/// 规则模板（SRS 第 5 节）。武器活动唤取必出当期武器，没有歪，不判断 UP。
pub fn rules() -> Vec<PoolRule> {
    let c = curve(0.008, Some(66), 0.06, 80);
    vec![
        rule("character", 5, c, fifty_fifty(0.5)),
        rule("weapon", 5, c, UpRule::None),
        rule("standard_character", 5, c, UpRule::None),
        rule("standard_weapon", 5, c, UpRule::None),
        rule("beginner", 5, curve(0.008, None, 0.0, 50), UpRule::None),
        rule("beginner_selector", 5, c, UpRule::None),
        rule("giveback_selector", 5, c, UpRule::None),
    ]
}

static URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https://aki-gm-resources(?:-oversea)?\.aki-game\.(?:com|net)/aki/gacha/index\.html#/record\?[^"\s\\]+"#)
        .expect("正则合法")
});

/// 国际服各服务器的 `svr_id` 与时区（分钟）。
const GLOBAL_SERVERS: [(&str, i32); 5] = [
    ("591d6af3a3090d8ea00d8f86cf6d7501", -300), // America
    ("6eb2a235b30d05efd77bedb5cf60999e", 60),   // Europe
    ("86d52186155b148b5c138ceb41be9650", 480),  // Asia
    ("919752ae5ea09c1ced910dd668a63ffb", 480),  // HMT
    ("10cd7254d57e58ae560b15d51e34b4c8", 480),  // SEA
];

/// 解析后的鸣潮抽卡链接。
#[derive(Clone, PartialEq, Eq)]
pub struct KuroUrl {
    /// 是否国际服。
    pub global: bool,
    /// UID（`player_id`）。
    pub player_id: String,
    svr_id: String,
    record_id: String,
    card_pool_id: String,
    lang: String,
}

impl std::fmt::Debug for KuroUrl {
    // record_id 相当于凭据，不能输出
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KuroUrl")
            .field("global", &self.global)
            .field("player_id", &Redacted::uid(&self.player_id))
            .finish_non_exhaustive()
    }
}

impl KuroUrl {
    /// 解析链接；不是鸣潮抽卡链接或缺少必要参数时返回 `None`。
    pub fn parse(raw: &str) -> Option<Self> {
        let url = url::Url::parse(raw.trim()).ok()?;
        let host = url.host_str()?;
        if !host.starts_with("aki-gm-resources") {
            return None;
        }
        let query = url
            .query()
            .filter(|q| !q.is_empty())
            .or_else(|| url.fragment()?.split_once('?').map(|(_, q)| q))?;
        let params: BTreeMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let get = |k: &str| params.get(k).cloned();
        Some(Self {
            global: host.contains("oversea"),
            player_id: get("player_id")?,
            svr_id: get("svr_id")?,
            record_id: get("record_id")?,
            card_pool_id: get("resources_id").or_else(|| get("gacha_id"))?,
            lang: get("lang").unwrap_or_else(|| "zh-Hans".into()),
        })
    }

    /// 服务器时区（分钟）：国服 UTC+8，国际服按 `svr_id`，未知时 UTC+8。
    pub fn tz_offset_min(&self) -> i32 {
        GLOBAL_SERVERS
            .iter()
            .find(|(id, _)| *id == self.svr_id)
            .map_or(480, |(_, tz)| *tz)
    }

    fn endpoint(&self) -> &'static str {
        if self.global {
            "https://gmserver-api.aki-game2.net/gacha/record/query"
        } else {
            "https://gmserver-api.aki-game2.com/gacha/record/query"
        }
    }

    fn body(&self, card_pool_type: u32) -> Value {
        json!({
            "playerId": self.player_id,
            "cardPoolId": self.card_pool_id,
            "cardPoolType": card_pool_type,
            "serverId": self.svr_id,
            "languageCode": self.lang,
            "recordId": self.record_id,
        })
    }
}

/// 游戏日志的候选位置：给的是文件就用它，是目录就按官方启动器、Steam、Epic 的目录结构找。
pub fn log_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_owned()];
    }
    let mut out = Vec::new();
    for root in [path.to_owned(), path.join("Wuthering Waves Game")] {
        out.push(root.join("Client/Saved/Logs/Client.log"));
        for sdk in ["KrPcSdk_Mainland", "KrPcSdk_Global"] {
            out.push(root.join(format!(
                "Client/Binaries/Win64/ThirdParty/{sdk}/KRSDKRes/KRSDKWebView/debug.log"
            )));
        }
    }
    out.retain(|p| p.is_file());
    out
}

/// 日志中最后一条抽卡链接。
pub fn last_url_in(log: &Path) -> Option<String> {
    let text = fs::read(log).ok()?;
    let text = String::from_utf8_lossy(&text);
    URL_RE
        .find_iter(&text)
        .last()
        .map(|m| m.as_str().to_owned())
}

#[derive(Deserialize)]
struct Response {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: Option<Vec<Value>>,
}

/// 鸣潮拉取器。
pub struct KuroFetcher {
    client: reqwest::Client,
    pacing: Pacing,
    endpoint: Option<String>,
}

impl Default for KuroFetcher {
    fn default() -> Self {
        Self::new(default_client(), Pacing::default())
    }
}

impl KuroFetcher {
    /// 指定客户端与请求节奏。
    pub fn new(client: reqwest::Client, pacing: Pacing) -> Self {
        Self {
            client,
            pacing,
            endpoint: None,
        }
    }

    /// 把所有请求发到指定地址（测试用）。
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    async fn query_once(&self, url: &KuroUrl, card_pool_type: u32) -> Result<Vec<Value>> {
        let endpoint = self.endpoint.as_deref().unwrap_or_else(|| url.endpoint());
        let resp = self
            .client
            .post(endpoint)
            .json(&url.body(card_pool_type))
            .send()
            .await
            .map_err(network_error)?;
        let body: Response = check_status(resp)?.json().await.map_err(network_error)?;
        match body.code {
            0 => Ok(body.data.unwrap_or_default()),
            _ if body.message.contains("频繁") || body.message.contains("frequent") => {
                Err(AppError::RateLimited)
            }
            code => {
                // 链接里的 record_id 过期后接口返回非 0；其他错误也只能请用户重新打开唤取记录
                tracing::warn!(code, message = %body.message, "鸣潮接口返回错误");
                Err(AppError::AuthExpired)
            }
        }
    }
}

/// 把一条接口记录转成导入记录。
fn decode(
    item: &Value,
    code: u32,
    location: String,
    tz: i32,
) -> std::result::Result<ImportedRecord, RecordFailure> {
    let fail = |r: &str| RecordFailure::new(location.clone(), r);
    let o = item.as_object().ok_or_else(|| fail("expected an object"))?;
    let text = |k: &str| o.get(k).and_then(gachanote_core::exchange::json_text);
    let name = text("name").ok_or_else(|| fail("missing name"))?;
    let rarity = text("qualityLevel")
        .and_then(|s| s.parse::<u8>().ok())
        .ok_or_else(|| fail("missing or invalid qualityLevel"))?;
    let time = text("time").ok_or_else(|| fail("missing time"))?;
    let pulled_at =
        gachanote_formats::local_to_utc_ms(&time, tz).ok_or_else(|| fail("invalid time"))?;
    let mut raw = item.clone();
    if let Some(m) = raw.as_object_mut() {
        // 接口记录里的 cardPoolType 是卡池名称，另存数字类型便于以后重新解析
        m.insert("cardPoolTypeId".into(), json!(code));
    }
    Ok(ImportedRecord {
        pool: PoolRef::Official(code.to_string()),
        banner_id: None,
        ext_id: None,
        item_id: text("resourceId"),
        item_name: name,
        item_type: text("resourceType"),
        rarity,
        pulled_at,
        seq: None,
        is_pull: true,
        raw,
        location,
    })
}

#[async_trait]
impl Fetcher for KuroFetcher {
    fn kind(&self) -> FetchKind {
        FetchKind::Url
    }

    /// 在各候选位置的日志里找最后一条抽卡链接。
    async fn discover(&self, env: &dyn PathProvider) -> Result<Vec<FetchSource>> {
        let game = GameId::from(GameId::WUWA);
        let mut out: Vec<FetchSource> = Vec::new();
        for path in env.game_paths(&game) {
            for log in log_files(&path) {
                let Some(url) = last_url_in(&log) else {
                    continue;
                };
                if out
                    .iter()
                    .all(|s| s.url.as_ref().map(|u| u.expose()) != Some(&url))
                {
                    out.push(FetchSource {
                        game: game.clone(),
                        url: Some(Redacted::url(url)),
                        account: None,
                    });
                }
            }
        }
        if out.is_empty() {
            Err(AppError::UrlNotFound { game })
        } else {
            Ok(out)
        }
    }

    async fn fetch(
        &self,
        src: &FetchSource,
        since: &dyn SinceLookup,
        progress: &dyn Progress,
        cancel: &CancelToken,
    ) -> Result<Vec<ImportedAccount>> {
        let url = src
            .url
            .as_ref()
            .and_then(|u| KuroUrl::parse(u.expose()))
            .ok_or_else(|| AppError::Invalid("not a Wuthering Waves gacha url".into()))?;
        let game = GameId::from(GameId::WUWA);
        let tz = url.tz_offset_min();
        let since = since.since(&game, &url.player_id)?;
        let mut acc = ImportedAccount::new(game, url.player_id.clone(), tz);
        acc.region = Some(if url.global { "global" } else { "cn" }.into());
        let mut fetched = 0u64;
        for (n, code) in QUERY_TYPES.into_iter().enumerate() {
            cancel.check()?;
            if n > 0 {
                tokio::time::sleep(self.pacing.delay()).await;
            }
            let list = with_retry(&self.pacing, cancel, || self.query_once(&url, code)).await?;
            let pool = pool_type(&code.to_string());
            let cursor = pool.as_ref().and_then(|p| since.get(p));
            // 接口按时间倒序返回；反转成正序，同一秒（十连）内的顺序才稳定
            for (i, item) in list.iter().enumerate().rev() {
                match decode(item, code, format!("{code}[{i}]"), tz) {
                    // 早于已有最新记录的不必再写入（同一秒的仍交给去重）
                    Ok(r) if cursor.is_some_and(|c| r.pulled_at < c.pulled_at) => {}
                    Ok(r) => {
                        acc.records.push(r);
                        fetched += 1;
                    }
                    Err(f) => acc.failures.push(f),
                }
            }
            progress.report(ProgressEvent {
                stage: Stage::Fetch,
                current: fetched,
                total: None,
                message: pool.map(|p| p.to_string()).unwrap_or_default(),
            });
        }
        Ok(vec![acc])
    }
}

/// 鸣潮插件。
pub struct Wuwa {
    rules: Vec<PoolRule>,
    fetcher: KuroFetcher,
}

impl Wuwa {
    /// 使用给定的拉取器。
    pub fn new(fetcher: KuroFetcher) -> Self {
        Self {
            rules: rules(),
            fetcher,
        }
    }
}

impl Default for Wuwa {
    fn default() -> Self {
        Self::new(KuroFetcher::default())
    }
}

impl GamePlugin for Wuwa {
    fn id(&self) -> GameId {
        GameId::WUWA.into()
    }

    fn info(&self) -> GameInfo {
        GameInfo {
            id: self.id(),
            vendor: Vendor::Kuro,
            theme_color: "#3FBFA8".into(),
            pool_types: crate::pool_order(&self.rules),
            fetch: Some(FetchKind::Url),
            formats: vec![FormatId::Gnj, FormatId::GnCsv],
        }
    }

    fn rules(&self) -> &[PoolRule] {
        &self.rules
    }

    fn pool_type(&self, official: &str) -> Option<PoolTypeId> {
        pool_type(official)
    }

    /// 6–12 位数字（目前所见为 9 位）。
    fn validate_uid(&self, uid: &str) -> bool {
        digits(uid, 6, 12)
    }

    fn fetcher(&self) -> Option<&dyn Fetcher> {
        Some(&self.fetcher)
    }
}

#[cfg(test)]
mod tests;
