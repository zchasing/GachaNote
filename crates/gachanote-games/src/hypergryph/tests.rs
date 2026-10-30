use gachanote_core::exchange::PoolRef;
use gachanote_core::model::Cursor;
use gachanote_core::plugin::{FullFetch, Since};
use gachanote_core::task::NoProgress;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::StandardPaths;

const TOKEN: &str = "account-token-0123456789";
const AK_UID: &str = "12345678";
const EF_UID: &str = "1000000005";

fn ok(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "status": 0, "msg": "OK", "data": data }))
}

fn game_ok(data: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({ "code": 0, "msg": "", "data": data }))
}

/// grant、角色列表与 U8 token 三个账号接口。
async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/user/oauth2/v2/grant"))
        .and(body_partial_json(json!({ "token": TOKEN, "type": 1 })))
        .respond_with(ok(json!({ "token": "oauth", "hgId": "1" })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/user/oauth2/v2/grant"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "status": 3, "msg": "登录已过期", "data": null })),
        )
        .with_priority(10)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/account/binding/v1/binding_list"))
        .and(query_param("token", "oauth"))
        .respond_with(ok(json!({ "list": [
            { "appCode": "arknights", "appName": "明日方舟", "bindingList": [
                { "uid": AK_UID, "isOfficial": true, "channelName": "官服", "nickName": "博士#1234" }
            ] },
            { "appCode": "endfield", "appName": "明日方舟：终末地", "bindingList": [
                { "uid": EF_UID, "channelName": "官服", "roles": [
                    { "serverId": "1", "serverName": "China", "roleId": "9", "nickName": "管理员", "level": 53 }
                ] }
            ] },
            { "appCode": "other", "bindingList": [{ "uid": "1" }] }
        ] })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/account/binding/v1/u8_token_by_uid"))
        .and(body_partial_json(json!({ "token": "oauth" })))
        .respond_with(ok(json!({ "token": "u8" })))
        .mount(&server)
        .await;
    server
}

fn client(server: &MockServer) -> HgClient {
    HgClient::new(reqwest::Client::new(), Pacing::immediate())
        .with_endpoints(HgEndpoints::all(&server.uri()))
}

fn source(game: &str, uid: &str) -> FetchSource {
    FetchSource {
        game: game.into(),
        url: None,
        token: Some(Redacted::secret(TOKEN.into())),
        uid: Some(uid.into()),
    }
}

#[test]
fn normalizes_pasted_tokens() {
    assert_eq!(
        normalize_token(&format!("  {TOKEN}\n")).as_deref(),
        Some(TOKEN)
    );
    let json = format!(r#"{{"code":0,"data":{{"content":"{TOKEN}"}},"msg":"接口调用成功"}}"#);
    assert_eq!(normalize_token(&json).as_deref(), Some(TOKEN));
    assert_eq!(
        normalize_token(&format!("\"{TOKEN}\"")).as_deref(),
        Some(TOKEN)
    );
    assert_eq!(normalize_token("short"), None);
    assert_eq!(normalize_token("has space inside the token"), None);
    assert_eq!(normalize_token(r#"{"code":3,"msg":"未登录"}"#), None);
    assert_eq!(normalize_token("{not json"), None);
}

#[test]
fn endpoints_and_games() {
    let e = HgEndpoints::all("http://127.0.0.1:1/");
    assert_eq!(e.account, "http://127.0.0.1:1");
    assert_eq!(e.web, e.endfield);
    assert!(HgEndpoints::default().account.starts_with("https://"));
    assert_eq!(HgGame::Arknights.id(), GameId::from(GameId::ARKNIGHTS));
    assert_eq!(HgGame::Endfield.id(), GameId::from(GameId::ENDFIELD));
    assert!(matches!(game_error(3, ""), AppError::AuthExpired));
    assert!(matches!(game_error(1, "token 无效"), AppError::AuthExpired));
    assert!(matches!(game_error(500, "busy"), AppError::Network(_)));
}

#[tokio::test]
async fn lists_roles_of_both_games() {
    let server = server().await;
    let hg = client(&server);
    let roles = hg.roles(TOKEN, &CancelToken::new()).await.unwrap();
    assert_eq!(roles.len(), 2);
    assert_eq!(roles[0].game, GameId::from(GameId::ARKNIGHTS));
    assert_eq!(roles[0].uid, AK_UID);
    assert_eq!(roles[0].nickname.as_deref(), Some("博士#1234"));
    assert_eq!(roles[0].channel.as_deref(), Some("官服"));
    assert_eq!(roles[1].uid, EF_UID);
    assert_eq!(roles[1].nickname.as_deref(), Some("管理员"));
    assert_eq!(roles[1].server_id.as_deref(), Some("1"));
    // token 失效
    let e = hg
        .roles("expired-token-0123456789", &CancelToken::new())
        .await;
    assert_eq!(e.unwrap_err(), AppError::AuthExpired);
}

fn ak_item(pool: &str, name: &str, rarity: u8, ts: i64, pos: u32) -> Value {
    json!({ "poolId": pool, "poolName": "寻访", "charId": format!("char_{name}"), "charName": name,
            "rarity": rarity, "isNew": false, "gachaTs": ts.to_string(), "pos": pos })
}

async fn mount_arknights(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/user/api/role/login"))
        .and(body_partial_json(json!({ "token": "u8" })))
        .respond_with(
            game_ok(json!({}))
                .insert_header("set-cookie", "ak-user-center=session1; Path=/; HttpOnly"),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user/api/role/info"))
        .and(header("x-role-token", "u8"))
        .and(header("cookie", "ak-user-center=session1"))
        .respond_with(
            game_ok(json!({ "uid": AK_UID, "name": "博士" }))
                .insert_header("set-cookie", "ak-user-center=session2; Path=/"),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user/api/inquiry/gacha/cate"))
        .and(header("cookie", "ak-user-center=session2"))
        .and(header("x-account-token", TOKEN))
        .respond_with(game_ok(json!([
            { "id": "anniver_fest", "name": "限定寻访\n庆典" },
            { "id": "normal", "name": "标准寻访" }
        ])))
        .mount(server)
        .await;
    // 限定：两页
    Mock::given(method("GET"))
        .and(path("/user/api/inquiry/gacha/history"))
        .and(query_param("category", "anniver_fest"))
        .and(query_param("pos", "1"))
        .and(query_param("gachaTs", "1761292800000"))
        .respond_with(game_ok(json!({ "list": [
            ak_item("LIMITED_37_0_1", "芬", 2, 1_761_200_000_000, 0),
        ], "hasMore": false })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user/api/inquiry/gacha/history"))
        .and(query_param("category", "anniver_fest"))
        .and(query_param("size", "10"))
        .and(header("x-role-token", "u8"))
        .respond_with(game_ok(json!({ "list": [
            ak_item("LIMITED_37_0_1", "能天使", 5, 1_761_292_800_000, 0),
            ak_item("LIMITED_37_0_1", "芬", 2, 1_761_292_800_000, 1),
        ], "hasMore": true })))
        .with_priority(5)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user/api/inquiry/gacha/history"))
        .and(query_param("category", "normal"))
        .respond_with(game_ok(json!({ "list": [
            ak_item("NORM_1", "玫兰莎", 2, 1_761_300_000_000, 0),
            { "poolId": "NORM_1", "charName": "坏", "rarity": 9, "gachaTs": "1761300000001", "pos": 0 },
        ], "hasMore": false })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn fetches_arknights_with_session_cookie() {
    let server = server().await;
    mount_arknights(&server).await;
    let f = HgFetcher::new(HgGame::Arknights, client(&server));
    assert_eq!(f.kind(), FetchKind::AccountToken);
    assert!(f
        .discover(&StandardPaths::from_env())
        .await
        .unwrap()
        .is_empty());
    let accs = f
        .fetch(
            &source("arknights", AK_UID),
            &FullFetch,
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(accs.len(), 1);
    let a = &accs[0];
    assert_eq!((a.uid.as_str(), a.region.as_deref()), (AK_UID, Some("cn")));
    assert_eq!(a.nickname.as_deref(), Some("博士#1234"));
    assert_eq!(a.records.len(), 4);
    let first = &a.records[0];
    assert_eq!(first.item_name, "能天使");
    assert_eq!(first.rarity, 6);
    assert_eq!(first.seq, Some(0));
    assert_eq!(first.pool, PoolRef::Official("LIMITED_37_0_1".into()));
    assert_eq!(first.ext_id, None);
    assert_eq!(a.records[1].seq, Some(1));
    assert_eq!(a.failures.len(), 1);
    assert_eq!(a.failures[0].reason, "invalid rarity");
}

#[tokio::test]
async fn arknights_incremental_stops_at_cursor() {
    let server = server().await;
    mount_arknights(&server).await;
    let f = HgFetcher::new(HgGame::Arknights, client(&server));
    let mut since = Since::new();
    since.insert(
        "limited".into(),
        Cursor {
            ext_id: None,
            pulled_at: 1_761_292_800_000,
            seq_in_sec: 0,
        },
    );
    let accs = f
        .fetch(
            &source("arknights", AK_UID),
            &since,
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    // 限定池只要比游标新的「芬」（同一时间 pos 1），不再翻第二页；标准池没有游标照常拉取
    let names: Vec<&str> = accs[0]
        .records
        .iter()
        .map(|r| r.item_name.as_str())
        .collect();
    assert_eq!(names, ["芬", "玫兰莎"]);
}

#[tokio::test]
async fn fetch_needs_token_uid_and_bound_role() {
    let server = server().await;
    let f = HgFetcher::new(HgGame::Arknights, client(&server));
    let cancel = CancelToken::new();
    let mut src = source("arknights", "999");
    let e = f
        .fetch(&src, &FullFetch, &NoProgress, &cancel)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "not_found");
    // 日志与错误信息里 UID 已脱敏
    assert!(!e.to_string().contains("999") || e.to_string().contains("***"));
    src.uid = None;
    let e = f
        .fetch(&src, &FullFetch, &NoProgress, &cancel)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid");
    src.token = None;
    let e = f
        .fetch(&src, &FullFetch, &NoProgress, &cancel)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "invalid");
}

fn ef_draw(pool: &str, name: &str, rarity: u8, ts: i64, seq: u32, free: bool) -> Value {
    json!({ "kind": "draw", "nameText": name, "poolId": pool, "poolName": "池", "charId": format!("chr_{name}"),
            "charName": name, "rarity": rarity, "isFree": free, "isNew": false,
            "gachaTs": ts.to_string(), "seqId": seq.to_string() })
}

async fn mount_endfield(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/record/char/meta"))
        .and(query_param("token", "u8"))
        .and(query_param("server_id", "1"))
        .respond_with(game_ok(json!({ "tabs": [
            { "key": "special", "label": "", "poolType": "E_CharacterGachaPoolType_Special" },
            { "key": "new", "label": "", "poolType": "E_CharacterGachaPoolType_New" }
        ], "beginnerPullCount": 40 })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/record/char/pool"))
        .respond_with(game_ok(
            json!([{ "poolId": "joint_1_2_2", "poolName": "辉光庆典" }]),
        ))
        .mount(server)
        .await;
    // 特殊池：第二页（seq_id = 213 之前）
    Mock::given(method("GET"))
        .and(path("/api/record/char"))
        .and(query_param("pool_type", "E_CharacterGachaPoolType_Special"))
        .and(query_param("seq_id", "213"))
        .respond_with(game_ok(json!({ "list": [
            ef_draw("special_1_0_1", "秋栗", 4, 1_770_000_000_000, 100, false),
        ], "hasMore": false })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/record/char"))
        .and(query_param("pool_type", "E_CharacterGachaPoolType_Special"))
        .respond_with(game_ok(json!({ "list": [
            ef_draw("special_1_0_1", "莱万汀", 6, 1_770_439_342_803, 212, false),
            { "kind": "gift_intel_book", "nameText": "寻访情报书", "poolId": "special_1_0_1",
              "gachaTs": "1770439342803", "seqId": "213" },
        ], "hasMore": true })))
        .with_priority(5)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/record/char"))
        .and(query_param("pool_type", "E_CharacterGachaPoolType_Joint"))
        .respond_with(game_ok(json!({ "list": [
            ef_draw("joint_1_2_2", "埃特拉", 5, 1_770_500_000_000, 3, true),
        ], "hasMore": false })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/record/char"))
        .respond_with(game_ok(json!({ "list": [], "hasMore": false })))
        .with_priority(10)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/record/weapon/pool"))
        .respond_with(game_ok(json!([
            { "poolId": "weponbox_1_0_1", "poolName": "熔铸申领", "poolType": "normal", "online": false }
        ])))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/record/weapon"))
        .and(query_param("pool_id", "weponbox_1_0_1"))
        .respond_with(game_ok(json!({ "list": [
            { "kind": "draw", "nameText": "寻路者道标", "poolId": "weponbox_1_0_1", "poolType": "normal",
              "poolVersion": null, "weaponId": "wpn_lance_0003", "weaponName": "寻路者道标",
              "weaponType": "E_WeaponType_Lance", "rarity": 4, "isNew": false,
              "gachaTs": "1769565182199", "seqId": "60" },
            { "kind": "draw", "poolId": "weponbox_1_0_1", "gachaTs": "1769565182199", "seqId": "59" },
        ], "hasMore": false })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn fetches_endfield_characters_and_weapons() {
    let server = server().await;
    mount_endfield(&server).await;
    let f = HgFetcher::new(HgGame::Endfield, client(&server));
    let accs = f
        .fetch(
            &source("endfield", EF_UID),
            &FullFetch,
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    let a = &accs[0];
    assert_eq!(a.nickname.as_deref(), Some("管理员"));
    let ids: Vec<&str> = a
        .records
        .iter()
        .filter_map(|r| r.ext_id.as_deref())
        .collect();
    assert_eq!(
        ids,
        [
            "special_1_0_1:draw:212",
            "special_1_0_1:gift_intel_book:213",
            "special_1_0_1:draw:100",
            "joint_1_2_2:draw:3",
            "weponbox_1_0_1:draw:60",
        ]
    );
    let gift = &a.records[1];
    assert_eq!(
        (gift.rarity, gift.is_pull, gift.item_name.as_str()),
        (1, false, "寻访情报书")
    );
    assert_eq!(
        a.records[0].pool,
        PoolRef::Official("char:special_1_0_1".into())
    );
    assert_eq!(a.records[0].rarity, 6);
    // 免费抽不计抽数
    assert!(!a.records[3].is_pull);
    let w = &a.records[4];
    assert_eq!(w.pool, PoolRef::Official("weapon:weponbox_1_0_1".into()));
    assert_eq!(w.item_type.as_deref(), Some("E_WeaponType_Lance"));
    assert_eq!(w.item_id.as_deref(), Some("wpn_lance_0003"));
    assert_eq!(a.failures.len(), 1);
}

#[tokio::test]
async fn endfield_incremental_and_expired_token() {
    let server = server().await;
    mount_endfield(&server).await;
    let f = HgFetcher::new(HgGame::Endfield, client(&server));
    let mut since = Since::new();
    since.insert(
        "character".into(),
        Cursor {
            ext_id: Some("special_1_0_1:draw:212".into()),
            pulled_at: 1_770_439_342_803,
            seq_in_sec: 0,
        },
    );
    let accs = f
        .fetch(
            &source("endfield", EF_UID),
            &since,
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap();
    // 同一时间的记录保留（由去重索引跳过），更早的那页不再请求
    let names: Vec<&str> = accs[0]
        .records
        .iter()
        .map(|r| r.item_name.as_str())
        .collect();
    assert_eq!(names, ["莱万汀", "寻访情报书", "埃特拉", "寻路者道标"]);

    let mut src = source("endfield", EF_UID);
    src.token = Some(Redacted::secret("expired-token-0123456789".into()));
    let e = f
        .fetch(&src, &FullFetch, &NoProgress, &CancelToken::new())
        .await
        .unwrap_err();
    assert_eq!(e, AppError::AuthExpired);
}

#[tokio::test]
async fn endfield_record_endpoint_error_maps_to_auth_expired() {
    let server = server().await;
    Mock::given(method("GET"))
        .and(path("/api/record/char/meta"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "code": 3, "msg": "token 已失效", "data": null })),
        )
        .mount(&server)
        .await;
    let f = HgFetcher::new(HgGame::Endfield, client(&server));
    let e = f
        .fetch(
            &source("endfield", EF_UID),
            &FullFetch,
            &NoProgress,
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(e, AppError::AuthExpired);
}

#[tokio::test]
async fn reads_token_with_login_cookies() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/account/info/hg"))
        .and(header("cookie", "ACCOUNT=abc"))
        .respond_with(game_ok(json!({ "content": TOKEN })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/account/info/hg"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "code": 3, "msg": "未登录", "data": null })),
        )
        .with_priority(10)
        .mount(&server)
        .await;
    let hg = client(&server);
    assert_eq!(
        hg.token_from_cookies("ACCOUNT=abc")
            .await
            .unwrap()
            .as_deref(),
        Some(TOKEN)
    );
    assert_eq!(hg.token_from_cookies("").await.unwrap(), None);
    assert_eq!(ts(Some(&json!("17"))), Some(17));
    assert_eq!(ts(Some(&json!(18))), Some(18));
    assert_eq!(ts(None), None);
}
