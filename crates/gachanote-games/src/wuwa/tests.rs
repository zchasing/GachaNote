use std::fs;

use gachanote_core::model::Cursor;
use gachanote_core::plugin::{FullFetch, Since};
use gachanote_core::task::NoProgress;
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::StandardPaths;

const CN: &str = "https://aki-gm-resources.aki-game.com/aki/gacha/index.html#/record?svr_id=76402e5b20be2c39f095a152090afddc&player_id=100000001&lang=zh-Hans&gacha_id=100001&gacha_type=1&svr_area=cn&record_id=rec&resources_id=res";
const EU: &str = "https://aki-gm-resources-oversea.aki-game.net/aki/gacha/index.html#/record?svr_id=6eb2a235b30d05efd77bedb5cf60999e&player_id=500000001&record_id=r&gacha_id=g";

fn source(url: &str) -> FetchSource {
    FetchSource {
        game: GameId::WUWA.into(),
        url: Some(Redacted::url(url.to_owned())),
        token: None,
        uid: None,
    }
}

fn rec(name: &str, quality: u8, time: &str) -> Value {
    json!({
        "cardPoolType": "角色精准调谐", "resourceId": 1404, "qualityLevel": quality,
        "resourceType": "角色", "name": name, "count": 1, "time": time,
    })
}

fn ok(list: Vec<Value>) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .set_body_json(json!({ "code": 0, "message": "success", "data": list }))
}

async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ok(vec![]))
        .with_priority(10)
        .mount(&server)
        .await;
    server
}

fn fetcher(server: &MockServer) -> KuroFetcher {
    KuroFetcher::new(reqwest::Client::new(), Pacing::immediate()).with_endpoint(server.uri())
}

#[test]
fn parses_urls() {
    let u = KuroUrl::parse(CN).unwrap();
    assert!(!u.global);
    assert_eq!(u.player_id, "100000001");
    assert_eq!(u.tz_offset_min(), 480);
    assert!(u.endpoint().ends_with(".com/gacha/record/query"));
    assert_eq!(u.body(3)["cardPoolType"], 3);
    assert_eq!(u.body(3)["cardPoolId"], "res");
    assert!(!format!("{u:?}").contains("rec"));

    let u = KuroUrl::parse(EU).unwrap();
    assert!(u.global);
    assert_eq!(u.tz_offset_min(), 60);
    assert_eq!(u.body(1)["cardPoolId"], "g");
    assert_eq!(u.body(1)["languageCode"], "zh-Hans");
    assert!(u.endpoint().contains("aki-game2.net"));

    for bad in [
        "https://example.com/#/record?player_id=1",
        "https://aki-gm-resources.aki-game.com/aki/gacha/index.html#/record?player_id=1",
        "https://aki-gm-resources.aki-game.com/aki/gacha/index.html",
        "nonsense",
    ] {
        assert!(KuroUrl::parse(bad).is_none(), "{bad}");
    }
    assert_eq!(crate::detect_url(CN).unwrap(), GameId::WUWA);
}

#[tokio::test]
async fn fetches_each_pool_type_oldest_first() {
    let server = server().await;
    Mock::given(body_partial_json(
        json!({ "cardPoolType": 1, "playerId": "100000001" }),
    ))
    .respond_with(ok(vec![
        rec("今汐", 5, "2024-06-28 12:00:01"),
        rec("秧秧", 4, "2024-06-28 12:00:00"),
        rec("秧秧", 4, "2024-06-28 12:00:00"),
        rec("坏", 4, "bad time"),
    ]))
    .mount(&server)
    .await;
    Mock::given(body_partial_json(json!({ "cardPoolType": 2 })))
        .respond_with(ok(vec![json!({ "qualityLevel": 3 }), json!(1)]))
        .mount(&server)
        .await;
    let accounts = fetcher(&server)
        .fetch(&source(CN), &FullFetch, &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    let a = &accounts[0];
    assert_eq!((a.uid.as_str(), a.tz_offset_min), ("100000001", 480));
    assert_eq!(a.region.as_deref(), Some("cn"));
    let names: Vec<_> = a.records.iter().map(|r| r.item_name.as_str()).collect();
    assert_eq!(names, ["秧秧", "秧秧", "今汐"]);
    let r = &a.records[2];
    assert_eq!(r.pool, PoolRef::Official("1".into()));
    assert_eq!((r.rarity, r.item_id.as_deref()), (5, Some("1404")));
    assert_eq!(r.raw["cardPoolTypeId"], 1);
    // 2024-06-28 12:00:01 UTC+8
    assert_eq!(r.pulled_at, 1_719_547_201_000);
    let reasons: Vec<_> = a.failures.iter().map(|f| f.reason.as_str()).collect();
    assert_eq!(
        reasons,
        ["invalid time", "expected an object", "missing name"]
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 7);

    // 增量：早于游标的记录不再返回
    let mut since = Since::new();
    since.insert(
        "character".into(),
        Cursor {
            ext_id: None,
            pulled_at: 1_719_547_201_000,
            seq_in_sec: 0,
        },
    );
    let accounts = fetcher(&server)
        .fetch(&source(CN), &since, &NoProgress, &CancelToken::new())
        .await
        .unwrap();
    assert_eq!(accounts[0].records.len(), 1);
}

#[tokio::test]
async fn errors() {
    let server = MockServer::start().await;
    Mock::given(body_partial_json(json!({ "recordId": "rec" })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "code": -1, "message": "请求参数错误" })),
        )
        .mount(&server)
        .await;
    Mock::given(body_partial_json(json!({ "recordId": "r" })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "code": 1, "message": "访问过于频繁" })),
        )
        .mount(&server)
        .await;
    let f = fetcher(&server);
    let e = f
        .fetch(&source(CN), &FullFetch, &NoProgress, &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(e, AppError::AuthExpired);
    let e = f
        .fetch(&source(EU), &FullFetch, &NoProgress, &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(e, AppError::RateLimited);
    let e = f
        .fetch(
            &source("https://x.com"),
            &FullFetch,
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid");
    let cancel = CancelToken::new();
    cancel.cancel();
    let e = f
        .fetch(&source(CN), &FullFetch, &NoProgress, &cancel)
        .await
        .unwrap_err();
    assert_eq!(e, AppError::Cancelled);
}

/// 三种安装方式的目录结构都能找到日志里最后一条链接。
#[tokio::test]
async fn discovers_url_in_three_layouts() {
    let dir = tempfile::tempdir().unwrap();
    let launcher = dir.path().join("Wuthering Waves");
    let steam = dir.path().join("steamapps/common/Wuthering Waves");
    let epic = dir.path().join("Epic Games/WutheringWavesj3oFh");
    let old = CN.replace("record_id=rec", "record_id=old");
    let logs = [
        (launcher.join("Wuthering Waves Game/Client/Saved/Logs/Client.log"), CN.to_owned()),
        (steam.join("Client/Saved/Logs/Client.log"), EU.to_owned()),
        (
            epic.join("Wuthering Waves Game/Client/Binaries/Win64/ThirdParty/KrPcSdk_Global/KRSDKRes/KRSDKWebView/debug.log"),
            EU.to_owned(),
        ),
    ];
    for (log, url) in &logs {
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(
            log,
            format!("[2024.06.28] LogTemp: {{\"url\":\"{old}\"}}\n[2024.06.28] LogTemp: {{\"url\":\"{url}\",\"x\":1}}\n"),
        )
        .unwrap();
    }
    let f = KuroFetcher::default();
    for (root, url) in [(&launcher, CN), (&steam, EU), (&epic, EU)] {
        let env = StandardPaths::default().with_extra(GameId::WUWA, root);
        let found = f.discover(&env).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].url.as_ref().unwrap().expose(), url);
    }
    // 直接给日志文件；同一条链接只返回一次
    let env = StandardPaths::default()
        .with_extra(GameId::WUWA, &logs[1].0)
        .with_extra(GameId::WUWA, &epic);
    assert_eq!(f.discover(&env).await.unwrap().len(), 1);
    let env = StandardPaths::default().with_extra(GameId::WUWA, dir.path());
    assert_eq!(
        f.discover(&env).await.unwrap_err(),
        AppError::UrlNotFound {
            game: GameId::WUWA.into()
        }
    );
    assert_eq!(f.kind(), FetchKind::Url);
}
