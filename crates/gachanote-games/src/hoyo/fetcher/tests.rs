use std::sync::Mutex;

use gachanote_core::model::Cursor;
use gachanote_core::plugin::FullFetch;
use gachanote_core::task::NoProgress;
use gachanote_core::GameId;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::StandardPaths;

const UID: &str = "100000001";

fn url(key: &str) -> String {
    format!("https://webstatic.mihoyo.com/hk4e/event/e20190909gacha-v3/index.html?authkey_ver=1&sign_type=2&auth_appid=webview_gacha&lang=zh-cn&region=cn_gf01&authkey={key}&game_biz=hk4e_cn#/log")
}

fn source(key: &str) -> FetchSource {
    FetchSource {
        game: GameId::GENSHIN.into(),
        url: Some(Redacted::url(url(key))),
        account: None,
    }
}

fn item(id: u64, gacha_type: &str, rank: &str) -> Value {
    json!({
        "uid": UID, "gacha_type": gacha_type, "item_id": "", "count": "1",
        "time": "2024-01-01 12:00:00", "name": format!("物品{id}"), "lang": "zh-cn",
        "item_type": "角色", "rank_type": rank, "id": id.to_string(),
    })
}

/// `ids` 从新到旧。
fn page(ids: impl IntoIterator<Item = u64>, gacha_type: &str) -> ResponseTemplate {
    let list: Vec<Value> = ids.into_iter().map(|i| item(i, gacha_type, "4")).collect();
    ResponseTemplate::new(200).set_body_json(json!({
        "retcode": 0, "message": "OK",
        "data": { "page": "1", "size": "20", "list": list, "region": "cn_gf01" }
    }))
}

fn retcode(code: i64, message: &str) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({ "retcode": code, "message": message, "data": null }))
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    // 其他卡池类型没有记录
    Mock::given(method("GET"))
        .and(path("/log"))
        .respond_with(page([], "200"))
        .with_priority(10)
        .mount(&server)
        .await;
    server
}

fn fetcher(server: &MockServer) -> HoyoFetcher {
    HoyoFetcher::with_client(
        HoyoGame::Genshin,
        reqwest::Client::new(),
        Pacing::immediate(),
    )
    .with_endpoint(format!("{}/log", server.uri()))
}

/// 301 两页：第一页 140..=121，第二页 120..=118（含一条缺星级的坏记录）。
async fn mount_character(server: &MockServer) {
    Mock::given(query_param("gacha_type", "301"))
        .and(query_param("end_id", "0"))
        .and(query_param("authkey", "good"))
        .and(query_param("size", "20"))
        .respond_with(page((121..=140).rev(), "301"))
        .mount(server)
        .await;
    let mut list: Vec<Value> = (118..=120).rev().map(|i| item(i, "400", "5")).collect();
    list.push(item(117, "301", ""));
    Mock::given(query_param("gacha_type", "301"))
        .and(query_param("end_id", "121"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "retcode": 0, "message": "OK", "data": { "list": list }
        })))
        .mount(server)
        .await;
}

#[derive(Default)]
struct Events(Mutex<Vec<(u64, String)>>);

impl Progress for Events {
    fn report(&self, e: ProgressEvent) {
        assert_eq!(e.stage, Stage::Fetch);
        self.0.lock().unwrap().push((e.current, e.message));
    }
}

#[tokio::test]
async fn full_fetch_pages_until_short_page() {
    let server = server().await;
    mount_character(&server).await;
    let events = Events::default();
    let accounts = fetcher(&server)
        .fetch(&source("good"), &FullFetch, &events, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    let a = &accounts[0];
    assert_eq!((a.uid.as_str(), a.tz_offset_min), (UID, 480));
    assert_eq!(a.region.as_deref(), Some("cn_gf01"));
    assert_eq!(a.lang.as_deref(), Some("zh-cn"));
    assert_eq!(a.records.len(), 23);
    assert_eq!(a.records[0].ext_id.as_deref(), Some("140"));
    assert_eq!(a.records[22].rarity, 5);
    assert_eq!(a.failures.len(), 1);
    assert_eq!(a.failures[0].location, "301#2[3]");
    // 每页报告一次进度：200 空页、301 两页、其余各一页空页
    let events = events.0.lock().unwrap();
    assert_eq!(events.len(), 6);
    assert_eq!(events[2], (23, "character".into()));
}

#[tokio::test]
async fn incremental_stops_at_cursor() {
    let server = server().await;
    mount_character(&server).await;
    let mut since = Since::new();
    since.insert(
        "character".into(),
        Cursor {
            ext_id: Some("130".into()),
            pulled_at: 0,
            seq_in_sec: 0,
        },
    );
    let accounts = fetcher(&server)
        .fetch(&source("good"), &since, &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    let ids: Vec<_> = accounts[0]
        .records
        .iter()
        .map(|r| r.ext_id.clone().unwrap())
        .collect();
    assert_eq!(ids.first().map(String::as_str), Some("140"));
    assert_eq!(ids.len(), 10);
    // 第二页没有请求
    let requests = server.received_requests().await.unwrap();
    assert!(!requests
        .iter()
        .any(|r| r.url.query().unwrap_or("").contains("end_id=121")));

    // 没有官方 ID 的游标按时间比较：全部记录都不比游标新
    let mut since = Since::new();
    since.insert(
        "character".into(),
        Cursor {
            ext_id: None,
            pulled_at: i64::MAX,
            seq_in_sec: 0,
        },
    );
    let accounts = fetcher(&server)
        .fetch(&source("good"), &since, &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert!(accounts[0].records.is_empty());
}

#[tokio::test]
async fn errors_are_mapped() {
    let server = MockServer::start().await;
    Mock::given(query_param("authkey", "expired"))
        .respond_with(retcode(-101, "authkey timeout"))
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "bad"))
        .respond_with(retcode(-1, "authkey error"))
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "busy"))
        .respond_with(retcode(-110, "visit too frequently"))
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "busy2"))
        .respond_with(retcode(-9, "too frequently"))
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "http429"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "http500"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "odd"))
        .respond_with(retcode(-502, "system error"))
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "html"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
        .mount(&server)
        .await;
    let f = fetcher(&server);
    let run = |key: &'static str| {
        let f = &f;
        async move {
            f.fetch(&source(key), &FullFetch, &NoProgress, &CancelToken::new())
                .await
                .unwrap_err()
        }
    };
    assert_eq!(run("expired").await, AppError::AuthExpired);
    assert_eq!(run("bad").await, AppError::AuthExpired);
    assert_eq!(run("busy").await, AppError::RateLimited);
    assert_eq!(run("busy2").await, AppError::RateLimited);
    assert_eq!(run("http429").await, AppError::RateLimited);
    assert_eq!(run("http500").await.code(), "network");
    assert_eq!(
        run("odd").await,
        AppError::Network("retcode -502: system error".into())
    );
    let html = run("html").await;
    assert_eq!(html.code(), "network");
    // 错误信息里不带链接参数
    assert!(!html.to_string().contains("authkey"));
    // 限流重试 3 次后放弃：共 4 次请求
    let busy = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.query().unwrap_or("").contains("authkey=busy&"))
        .count();
    assert_eq!(busy, 4);
}

#[tokio::test]
async fn rate_limit_then_success() {
    let server = server().await;
    Mock::given(query_param("gacha_type", "200"))
        .respond_with(retcode(-110, "visit too frequently"))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    let accounts = fetcher(&server)
        .fetch(
            &source("good"),
            &FullFetch,
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert!(accounts.is_empty());
}

#[tokio::test]
async fn invalid_source_and_cancel() {
    let server = server().await;
    let f = fetcher(&server);
    let none = FetchSource {
        game: GameId::GENSHIN.into(),
        url: None,
        account: None,
    };
    let other = FetchSource {
        url: Some(Redacted::url(
            "https://webstatic.mihoyo.com/x/index.html?authkey=1&game_biz=hkrpg_cn".into(),
        )),
        ..none.clone()
    };
    for src in [none, other] {
        let e = f
            .fetch(&src, &FullFetch, &NoProgress, &CancelToken::new())
            .await
            .unwrap_err();
        assert_eq!(e.code(), "invalid");
    }
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        f.fetch(&source("good"), &FullFetch, &NoProgress, &cancel)
            .await
            .unwrap_err(),
        AppError::Cancelled
    );
    assert_eq!(f.kind(), FetchKind::Url);
    let _ = HoyoFetcher::new(HoyoGame::Zzz);
}

/// 写一个只有缓存的数据目录，缓存中依次出现 `keys` 对应的链接（最后的最新）。
fn data_dir(root: &std::path::Path, keys: &[&str]) -> std::path::PathBuf {
    let data = root.join("YuanShen_Data");
    let cache = data.join("webCaches/2.30.0.0/Cache/Cache_Data");
    std::fs::create_dir_all(&cache).unwrap();
    let blob: Vec<u8> = keys
        .iter()
        .flat_map(|k| {
            let mut b = b"1/0/".to_vec();
            b.extend(url(k).as_bytes());
            b.push(0);
            b
        })
        .collect();
    std::fs::write(cache.join("data_2"), blob).unwrap();
    data
}

#[tokio::test]
async fn discover_returns_newest_valid_url() {
    let server = server().await;
    Mock::given(query_param("authkey", "expired"))
        .respond_with(retcode(-101, "authkey timeout"))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(query_param("authkey", "boom"))
        .respond_with(retcode(-1, "system busy"))
        .with_priority(1)
        .mount(&server)
        .await;
    let f = fetcher(&server);
    let dir = tempfile::tempdir().unwrap();

    let data = data_dir(&dir.path().join("a"), &["good", "expired"]);
    let env = StandardPaths::default().with_extra(GameId::GENSHIN, &data);
    let found = f.discover(&env).await.unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0]
        .url
        .as_ref()
        .unwrap()
        .expose()
        .contains("authkey=good"));

    let data = data_dir(&dir.path().join("b"), &["expired"]);
    let env = StandardPaths::default().with_extra(GameId::GENSHIN, &data);
    assert_eq!(f.discover(&env).await.unwrap_err(), AppError::AuthExpired);

    let data = data_dir(&dir.path().join("c"), &["boom"]);
    let env = StandardPaths::default().with_extra(GameId::GENSHIN, &data);
    assert_eq!(f.discover(&env).await.unwrap_err().code(), "network");

    let env = StandardPaths::default().with_extra(GameId::GENSHIN, dir.path());
    assert_eq!(
        f.discover(&env).await.unwrap_err(),
        AppError::UrlNotFound {
            game: GameId::GENSHIN.into()
        }
    );
}

#[test]
fn id_ordering() {
    assert_eq!(cmp_id("99", "100"), Ordering::Less);
    assert_eq!(cmp_id("101", "100"), Ordering::Greater);
    assert_eq!(cmp_id("100", "100"), Ordering::Equal);
}
