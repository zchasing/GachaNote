//! 明日方舟：角色凭证登录官网拿到会话 Cookie，按寻访类别用 `pos + gachaTs` 从新到旧分页。

use gachanote_core::exchange::{
    json_text, ImportedAccount, ImportedRecord, PoolRef, RecordFailure,
};
use gachanote_core::task::{ProgressEvent, Stage};
use gachanote_core::Result;
use reqwest::header::{HeaderMap, COOKIE, SET_COOKIE};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{ts, Ctx, GameResp};
use crate::net::{check_status, network_error, with_retry};

/// 每页条数（与官网一致）。
const PAGE_SIZE: &str = "10";

#[derive(Deserialize)]
struct Category {
    id: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPage {
    #[serde(default)]
    list: Vec<Value>,
    #[serde(default)]
    has_more: bool,
}

/// 收集响应里的 `Set-Cookie`，合并成请求用的 `Cookie` 头。
#[derive(Default)]
struct Jar(Vec<(String, String)>);

impl Jar {
    fn absorb(&mut self, headers: &HeaderMap) {
        for v in headers.get_all(SET_COOKIE) {
            let Ok(v) = v.to_str() else { continue };
            let pair = v.split(';').next().unwrap_or_default();
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            let name = name.trim().to_owned();
            self.0.retain(|(n, _)| n != &name);
            self.0.push((name, value.trim().to_owned()));
        }
    }

    fn header(&self) -> String {
        self.0
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// 发请求，记下 Cookie，检查业务错误码。
async fn call(
    ctx: &Ctx<'_>,
    jar: &mut Jar,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<()> {
    let headers = with_retry(&ctx.hg.pacing, ctx.cancel, || {
        let req = build();
        async move {
            let resp = check_status(req.send().await.map_err(network_error)?)?;
            let headers = resp.headers().clone();
            let body: GameResp<Value> = resp.json().await.map_err(network_error)?;
            body.into_data().map(|_| headers)
        }
    })
    .await?;
    jar.absorb(&headers);
    Ok(())
}

pub(crate) async fn fetch(
    ctx: &Ctx<'_>,
    account_token: &str,
    role_token: &str,
    uid: &str,
    acc: &mut ImportedAccount,
) -> Result<()> {
    let base = &ctx.hg.endpoints.arknights;
    let client = &ctx.hg.client;
    let mut jar = Jar::default();
    let login = format!("{base}/user/api/role/login");
    let body = json!({ "token": role_token, "source_from": "", "share_type": "", "share_by": "" });
    call(ctx, &mut jar, || client.post(&login).json(&body)).await?;
    let info = format!("{base}/user/api/role/info");
    let cookie = jar.header();
    call(ctx, &mut jar, || {
        client
            .get(&info)
            .query(&[("source_from", ""), ("share_type", ""), ("share_by", "")])
            .header("x-role-token", role_token)
            .header(COOKIE, &cookie)
    })
    .await?;
    let cookie = jar.header();
    let authed = |req: reqwest::RequestBuilder| {
        req.header("x-account-token", account_token)
            .header("x-role-token", role_token)
            .header(COOKIE, &cookie)
    };

    ctx.pause().await;
    let cate_url = format!("{base}/user/api/inquiry/gacha/cate");
    let categories: Vec<Category> = ctx
        .get(|| authed(client.get(&cate_url).query(&[("uid", uid)])))
        .await?;
    let history_url = format!("{base}/user/api/inquiry/gacha/history");
    let mut fetched = 0u64;
    for cat in categories {
        let Some(category) = json_text(&cat.id) else {
            continue;
        };
        let mut cursor: Option<(String, String)> = None;
        let mut page_no = 0u32;
        loop {
            ctx.cancel.check()?;
            ctx.pause().await;
            page_no += 1;
            let page: HistoryPage = ctx
                .get(|| {
                    let mut q = vec![
                        ("uid", uid.to_owned()),
                        ("category", category.clone()),
                        ("size", PAGE_SIZE.to_owned()),
                    ];
                    if let Some((pos, ts)) = &cursor {
                        q.push(("pos", pos.clone()));
                        q.push(("gachaTs", ts.clone()));
                    }
                    authed(client.get(&history_url).query(&q))
                })
                .await?;
            let mut reached = false;
            for (i, item) in page.list.iter().enumerate() {
                match record(item, format!("{category}#{page_no}[{i}]")) {
                    Ok(rec) => {
                        let pool =
                            crate::arknights::pool_type(&rec.banner_id.clone().unwrap_or_default());
                        let old = pool.and_then(|p| ctx.since.get(&p)).is_some_and(|c| {
                            (rec.pulled_at, rec.seq.unwrap_or(0)) <= (c.pulled_at, c.seq_in_sec)
                        });
                        if old {
                            reached = true;
                            continue;
                        }
                        acc.records.push(rec);
                        fetched += 1;
                    }
                    Err(f) => acc.failures.push(f),
                }
            }
            ctx.progress.report(ProgressEvent {
                stage: Stage::Fetch,
                current: fetched,
                total: None,
                message: category.clone(),
            });
            let last = page
                .list
                .last()
                .and_then(|v| Some((json_text(v.get("pos")?)?, json_text(v.get("gachaTs")?)?)));
            if reached || !page.has_more || last.is_none() || last == cursor {
                break;
            }
            cursor = last;
        }
    }
    Ok(())
}

/// 一条寻访记录。接口的 `rarity` 为 0–5，对应 1–6 星。
fn record(item: &Value, location: String) -> std::result::Result<ImportedRecord, RecordFailure> {
    let fail = |r: &str| RecordFailure::new(location.clone(), r);
    let text = |k: &str| item.get(k).and_then(json_text).filter(|s| !s.is_empty());
    let pool_id = text("poolId").ok_or_else(|| fail("missing poolId"))?;
    let rarity = item
        .get("rarity")
        .and_then(Value::as_u64)
        .filter(|r| *r <= 5)
        .ok_or_else(|| fail("invalid rarity"))?;
    let pulled_at = ts(item.get("gachaTs")).ok_or_else(|| fail("invalid gachaTs"))?;
    let pos = item
        .get("pos")
        .and_then(Value::as_u64)
        .and_then(|p| u32::try_from(p).ok())
        .unwrap_or(0);
    Ok(ImportedRecord {
        pool: PoolRef::Official(pool_id.clone()),
        banner_id: Some(pool_id),
        ext_id: None,
        item_id: text("charId"),
        item_name: text("charName").ok_or_else(|| fail("missing charName"))?,
        item_type: None,
        rarity: u8::try_from(rarity + 1).map_err(|_| fail("invalid rarity"))?,
        pulled_at,
        seq: Some(pos),
        is_pull: true,
        raw: item.clone(),
        location,
    })
}
