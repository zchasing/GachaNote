//! 终末地：角色池按 `pool_type`、武器池按 `pool_id` 逐池查询，用 `seqId` 从新到旧分页。
//!
//! 记录的写法与 arknights-gacha-export 的 CSV 导入一致（卡池代码 `<char|weapon>:<poolId>`，
//! 官方 ID `poolId:kind:seqId`），所以拉取和导入的同一条记录能互相去重。

use gachanote_core::exchange::{
    json_text, ImportedAccount, ImportedRecord, PoolRef, RecordFailure,
};
use gachanote_core::task::{ProgressEvent, Stage};
use gachanote_core::Result;
use serde::Deserialize;
use serde_json::Value;

use super::{ts, Ctx, HgRole};

/// 不论元数据接口返回什么都要查询的角色池类型（`tabs` 不保证包含新手池）。
const CHAR_POOL_TYPES: [&str; 4] = [
    "E_CharacterGachaPoolType_Special",
    "E_CharacterGachaPoolType_Rerun",
    "E_CharacterGachaPoolType_Standard",
    "E_CharacterGachaPoolType_Beginner",
];
/// 联合池：当前开放的特殊池列表里有 `joint_` 开头的卡池时查询。
const JOINT: &str = "E_CharacterGachaPoolType_Joint";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordPage {
    #[serde(default)]
    list: Vec<Value>,
    #[serde(default)]
    has_more: bool,
}

/// 角色池还是武器池。
#[derive(Clone, Copy)]
enum Kind {
    Char,
    Weapon,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Char => "char",
            Self::Weapon => "weapon",
        }
    }
}

pub(crate) async fn fetch(
    ctx: &Ctx<'_>,
    role_token: &str,
    role: &HgRole,
    acc: &mut ImportedAccount,
) -> Result<()> {
    let base = &ctx.hg.endpoints.endfield;
    let client = &ctx.hg.client;
    let server = role.server_id.clone().unwrap_or_else(|| "1".into());
    let common = [
        ("lang", "zh-cn"),
        ("token", role_token),
        ("server_id", server.as_str()),
    ];

    // 角色池类型：元数据 tabs + 固定的四种 + 联合池；这两个发现接口失败时不影响固定类型
    let mut types: Vec<String> = Vec::new();
    let meta_url = format!("{base}/api/record/char/meta");
    match ctx
        .get::<Value>(|| client.get(&meta_url).query(&common))
        .await
    {
        Ok(meta) => types.extend(
            meta.get("tabs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|t| t.get("poolType").and_then(Value::as_str))
                .map(str::to_owned),
        ),
        Err(e) if e.code() == "network" => tracing::debug!(error = %e, "终末地角色池元数据不可用"),
        Err(e) => return Err(e),
    }
    for t in CHAR_POOL_TYPES {
        types.push(t.into());
    }
    ctx.pause().await;
    let pool_url = format!("{base}/api/record/char/pool");
    match ctx
        .get::<Vec<Value>>(|| client.get(&pool_url).query(&common))
        .await
    {
        Ok(pools) => {
            if pools.iter().any(|p| {
                p.get("poolId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id.starts_with("joint_"))
            }) {
                types.push(JOINT.into());
            }
        }
        Err(e) if e.code() == "network" => {
            tracing::debug!(error = %e, "终末地特殊角色池列表不可用")
        }
        Err(e) => return Err(e),
    }
    let mut seen = Vec::new();
    types.retain(|t| {
        let new = !seen.contains(t);
        seen.push(t.clone());
        new
    });

    let mut fetched = 0u64;
    let char_url = format!("{base}/api/record/char");
    for t in &types {
        pages(
            ctx,
            Kind::Char,
            ("pool_type", t),
            &char_url,
            &common,
            acc,
            &mut fetched,
        )
        .await?;
    }

    ctx.pause().await;
    let weapon_pools_url = format!("{base}/api/record/weapon/pool");
    let weapon_pools: Vec<Value> = ctx
        .get(|| client.get(&weapon_pools_url).query(&common))
        .await?;
    let weapon_url = format!("{base}/api/record/weapon");
    for pool in weapon_pools {
        let Some(id) = pool.get("poolId").and_then(json_text) else {
            continue;
        };
        pages(
            ctx,
            Kind::Weapon,
            ("pool_id", &id),
            &weapon_url,
            &common,
            acc,
            &mut fetched,
        )
        .await?;
    }
    Ok(())
}

/// 拉取一个卡池的全部页，遇到已有记录（早于该卡池类型的游标）即停止。
async fn pages(
    ctx: &Ctx<'_>,
    kind: Kind,
    key: (&str, &str),
    url: &str,
    common: &[(&str, &str)],
    acc: &mut ImportedAccount,
    fetched: &mut u64,
) -> Result<()> {
    let client = &ctx.hg.client;
    let mut seq_id: Option<String> = None;
    let mut page_no = 0u32;
    loop {
        ctx.cancel.check()?;
        ctx.pause().await;
        page_no += 1;
        let page: RecordPage = ctx
            .get(|| {
                let mut req = client.get(url).query(common).query(&[key]);
                if let Some(s) = &seq_id {
                    req = req.query(&[("seq_id", s)]);
                }
                req
            })
            .await?;
        let mut reached = false;
        for (i, item) in page.list.iter().enumerate() {
            match record(kind, item, format!("{}#{page_no}[{i}]", key.1)) {
                Ok(rec) => {
                    let code = match &rec.pool {
                        PoolRef::Official(c) => c.clone(),
                        PoolRef::Normalized(p) => p.to_string(),
                    };
                    let pool = crate::endfield::pool_type(&code);
                    let old = pool
                        .and_then(|p| ctx.since.get(&p))
                        .is_some_and(|c| rec.pulled_at < c.pulled_at);
                    if old {
                        reached = true;
                        continue;
                    }
                    acc.records.push(rec);
                    *fetched += 1;
                }
                Err(f) => acc.failures.push(f),
            }
        }
        ctx.progress.report(ProgressEvent {
            stage: Stage::Fetch,
            current: *fetched,
            total: None,
            message: key.1.to_owned(),
        });
        // 下一页的游标取过滤前原始列表的最后一条（可能是赠送条目）
        let last = page
            .list
            .last()
            .and_then(|v| v.get("seqId"))
            .and_then(json_text);
        if reached || !page.has_more || last.is_none() || last == seq_id {
            break;
        }
        seq_id = last;
    }
    Ok(())
}

/// 一条记录：抽取（`kind = draw`）或赠送条目（`gift_*`，不计抽数，记为 1 星）。
fn record(
    kind: Kind,
    item: &Value,
    location: String,
) -> std::result::Result<ImportedRecord, RecordFailure> {
    let fail = |r: &str| RecordFailure::new(location.clone(), r);
    let text = |k: &str| item.get(k).and_then(json_text).filter(|s| !s.is_empty());
    let entry = text("kind").unwrap_or_else(|| "draw".into());
    let pool_id = text("poolId").ok_or_else(|| fail("missing poolId"))?;
    let seq_id = text("seqId").ok_or_else(|| fail("missing seqId"))?;
    let pulled_at = ts(item.get("gachaTs")).ok_or_else(|| fail("invalid gachaTs"))?;
    let is_draw = entry == "draw";
    let (id_key, name_key) = match kind {
        Kind::Char => ("charId", "charName"),
        Kind::Weapon => ("weaponId", "weaponName"),
    };
    let rarity = if is_draw {
        item.get("rarity")
            .and_then(Value::as_u64)
            .and_then(|r| u8::try_from(r).ok())
            .filter(|r| (1..=6).contains(r))
            .ok_or_else(|| fail("invalid rarity"))?
    } else {
        1
    };
    let name = if is_draw {
        text(name_key).or_else(|| text("nameText"))
    } else {
        text("nameText")
    };
    let is_free = item.get("isFree").and_then(Value::as_bool).unwrap_or(false);
    Ok(ImportedRecord {
        pool: PoolRef::Official(format!("{}:{pool_id}", kind.as_str())),
        banner_id: Some(pool_id.clone()),
        ext_id: Some(format!("{pool_id}:{entry}:{seq_id}")),
        item_id: text(id_key),
        item_name: name.ok_or_else(|| fail("missing name"))?,
        item_type: match kind {
            Kind::Weapon => text("weaponType"),
            Kind::Char => None,
        },
        rarity,
        pulled_at,
        seq: None,
        is_pull: is_draw && !is_free,
        raw: item.clone(),
        location,
    })
}
