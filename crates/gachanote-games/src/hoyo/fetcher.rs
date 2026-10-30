//! 米哈游抽卡记录拉取器：按卡池类型、用 `end_id` 游标从新到旧分页请求，
//! 遇到已有的最新记录即停止（增量），或拉到尽头（全量）。

use std::cmp::Ordering;
use std::collections::BTreeMap;

use async_trait::async_trait;
use gachanote_core::exchange::ImportedAccount;
use gachanote_core::plugin::{FetchKind, FetchSource, Fetcher, PathProvider, Since, SinceLookup};
use gachanote_core::task::{CancelToken, Progress, ProgressEvent, Stage};
use gachanote_core::{AppError, Redacted, Result};
use serde::Deserialize;
use serde_json::Value;

use super::{cache_urls, data_dirs, tz_of_uid, HoyoGame, HoyoUrl};
use crate::net::{check_status, default_client, network_error, with_retry, Pacing};

/// 每页条数（接口上限 20）。
const PAGE_SIZE: u32 = 20;
/// 自动查找时最多校验几条缓存中的链接。
const MAX_PROBES: usize = 5;

#[derive(Deserialize)]
struct Response {
    retcode: i64,
    #[serde(default)]
    message: String,
    data: Option<Data>,
}

#[derive(Deserialize)]
struct Data {
    #[serde(default)]
    list: Vec<Value>,
}

/// 官方记录 ID 的比较：数字字符串先比长度再比字典序。
fn cmp_id(a: &str, b: &str) -> Ordering {
    (a.len(), a).cmp(&(b.len(), b))
}

/// 米哈游拉取器。
pub struct HoyoFetcher {
    game: HoyoGame,
    client: reqwest::Client,
    pacing: Pacing,
    endpoint: Option<String>,
}

impl HoyoFetcher {
    /// 默认客户端与请求节奏。
    pub fn new(game: HoyoGame) -> Self {
        Self::with_client(game, default_client(), Pacing::default())
    }

    /// 指定客户端（代理设置）与请求节奏。
    pub fn with_client(game: HoyoGame, client: reqwest::Client, pacing: Pacing) -> Self {
        Self {
            game,
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

    fn parse(&self, src: &FetchSource) -> Result<HoyoUrl> {
        let raw = src
            .url
            .as_ref()
            .ok_or_else(|| AppError::Invalid("missing gacha url".into()))?;
        HoyoUrl::parse(raw.expose())
            .filter(|u| u.game == self.game)
            .ok_or_else(|| AppError::Invalid("not a gacha url of this game".into()))
    }

    /// 请求一页，不重试。
    async fn page_once(
        &self,
        url: &HoyoUrl,
        gacha_type: &str,
        end_id: &str,
        size: u32,
    ) -> Result<Vec<Value>> {
        let endpoint = self
            .endpoint
            .as_deref()
            .unwrap_or_else(|| self.game.endpoint(url.global, gacha_type));
        let resp = self
            .client
            .get(endpoint)
            .query(&url.query(gacha_type, end_id, size))
            .send()
            .await
            .map_err(network_error)?;
        let body: Response = check_status(resp)?.json().await.map_err(network_error)?;
        match body.retcode {
            0 => Ok(body.data.map(|d| d.list).unwrap_or_default()),
            -100 | -101 => Err(AppError::AuthExpired),
            -110 => Err(AppError::RateLimited),
            _ if body.message.contains("authkey") => Err(AppError::AuthExpired),
            _ if body.message.contains("frequently") => Err(AppError::RateLimited),
            code => Err(AppError::Network(format!(
                "retcode {code}: {}",
                body.message
            ))),
        }
    }

    /// 请求一页，网络错误与限流时重试。
    async fn page(
        &self,
        url: &HoyoUrl,
        gacha_type: &str,
        end_id: &str,
        size: u32,
        cancel: &CancelToken,
    ) -> Result<Vec<Value>> {
        with_retry(&self.pacing, cancel, || {
            self.page_once(url, gacha_type, end_id, size)
        })
        .await
    }

    /// 校验链接是否可用：请求常驻池一条记录。
    async fn probe(&self, url: &HoyoUrl) -> Result<()> {
        let first = self.game.query_types()[0];
        self.page(url, first, "0", 1, &CancelToken::new())
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl Fetcher for HoyoFetcher {
    fn kind(&self) -> FetchKind {
        FetchKind::Url
    }

    /// 在缓存中找链接，逐条校验，返回第一条可用的。
    /// 一条都没有时为 [`AppError::UrlNotFound`]；都已过期时为 [`AppError::AuthExpired`]。
    async fn discover(&self, env: &dyn PathProvider) -> Result<Vec<FetchSource>> {
        let id = self.game.id();
        let mut candidates = Vec::new();
        for path in env.game_paths(&id) {
            for dir in data_dirs(self.game, &path) {
                for u in cache_urls(self.game, &dir) {
                    if !candidates.contains(&u) {
                        candidates.push(u);
                    }
                }
            }
        }
        if candidates.is_empty() {
            return Err(AppError::UrlNotFound { game: id });
        }
        for raw in candidates.into_iter().take(MAX_PROBES) {
            let Some(url) = HoyoUrl::parse(&raw) else {
                continue;
            };
            match self.probe(&url).await {
                Ok(()) => {
                    return Ok(vec![FetchSource {
                        game: id,
                        url: Some(Redacted::url(raw)),
                        token: None,
                        uid: None,
                    }])
                }
                Err(AppError::AuthExpired) => {
                    tracing::debug!(url = %Redacted::url(&raw), "缓存中的链接已过期");
                }
                Err(e) => return Err(e),
            }
        }
        Err(AppError::AuthExpired)
    }

    async fn fetch(
        &self,
        src: &FetchSource,
        since: &dyn SinceLookup,
        progress: &dyn Progress,
        cancel: &CancelToken,
    ) -> Result<Vec<ImportedAccount>> {
        let url = self.parse(src)?;
        let game = self.game.id();
        let mut accounts: BTreeMap<String, ImportedAccount> = BTreeMap::new();
        let mut cursors: BTreeMap<String, Since> = BTreeMap::new();
        let mut fetched = 0u64;
        let mut first_request = true;

        for gacha_type in self.game.query_types() {
            let pool = self.game.pool_type(gacha_type);
            let mut end_id = String::from("0");
            let mut page_no = 0u32;
            loop {
                cancel.check()?;
                if !first_request {
                    tokio::time::sleep(self.pacing.delay()).await;
                }
                first_request = false;
                page_no += 1;
                let list = self
                    .page(&url, gacha_type, &end_id, PAGE_SIZE, cancel)
                    .await?;
                let full_page = list.len() >= PAGE_SIZE as usize;
                let mut reached = false;
                for (i, item) in list.iter().enumerate() {
                    let obj = item.as_object();
                    let field = |k: &str| {
                        obj.and_then(|o| o.get(k))
                            .and_then(gachanote_core::exchange::json_text)
                    };
                    if let Some(id) = field("id") {
                        end_id = id;
                    }
                    let Some(uid) = field("uid") else {
                        continue;
                    };
                    if !cursors.contains_key(&uid) {
                        cursors.insert(uid.clone(), since.since(&game, &uid)?);
                    }
                    let tz = url
                        .tz_offset_min()
                        .unwrap_or_else(|| tz_of_uid(self.game, &uid));
                    let acc = accounts.entry(uid.clone()).or_insert_with(|| {
                        let mut a = ImportedAccount::new(game.clone(), uid.clone(), tz);
                        a.region = (!url.region.is_empty()).then(|| url.region.clone());
                        a
                    });
                    if acc.lang.is_none() {
                        acc.lang = field("lang");
                    }
                    let location = format!("{gacha_type}#{page_no}[{i}]");
                    let rec = match gachanote_formats::uigf::decode_record(
                        item,
                        location,
                        tz,
                        self.game.banner_key(),
                    ) {
                        Ok(r) => r,
                        Err(f) => {
                            acc.failures.push(f);
                            continue;
                        }
                    };
                    let cursor = pool.as_ref().and_then(|p| cursors[&uid].get(p));
                    let old = cursor.is_some_and(|c| match (&c.ext_id, &rec.ext_id) {
                        (Some(c), Some(r)) => cmp_id(r, c) != Ordering::Greater,
                        _ => rec.pulled_at < c.pulled_at,
                    });
                    if old {
                        reached = true;
                        continue;
                    }
                    acc.records.push(rec);
                    fetched += 1;
                }
                progress.report(ProgressEvent {
                    stage: Stage::Fetch,
                    current: fetched,
                    total: None,
                    message: pool.as_ref().map(ToString::to_string).unwrap_or_default(),
                });
                if reached || !full_page {
                    break;
                }
            }
        }
        Ok(accounts.into_values().collect())
    }
}

#[cfg(test)]
mod tests;
