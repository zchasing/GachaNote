//! 鹰角账号 token 拉取（明日方舟、终末地，T-51，架构文档 7.3）。
//!
//! 一个鹰角账号 token 可以拉取账号下两款游戏的全部角色：
//!
//! 1. `as.hypergryph.com` 用账号 token 换 OAuth token（grant）；
//! 2. `binding-api-account-prod.hypergryph.com` 列出绑定的游戏角色，并按 UID 换取角色凭证（U8 token）；
//! 3. 明日方舟：`ak.hypergryph.com` 用角色凭证登录拿到会话 Cookie，再按寻访类别用 `pos + gachaTs` 分页；
//! 4. 终末地：`ef-webview.hypergryph.com` 直接用角色凭证查询，角色池按 `pool_type`、武器池按 `pool_id`，
//!    用 `seqId` 分页。
//!
//! 接口没有公开文档，只参考 arknights-gacha-export 的 API.md 描述的行为（该项目没有许可证，不复制代码）。
//! 账号 token 由调用方传入；OAuth token 与角色凭证只在内存里使用，不保存、不写日志。

mod arknights;
mod endfield;

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use gachanote_core::exchange::{json_text, ImportedAccount};
use gachanote_core::plugin::{FetchKind, FetchSource, Fetcher, PathProvider, SinceLookup};
use gachanote_core::task::{CancelToken, Progress};
use gachanote_core::{AppError, GameId, Redacted, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::net::{check_status, network_error, with_retry, Pacing};

/// 鹰角官网的应用代码（grant 接口用）。
const APP_CODE: &str = "be36d44aa36bfb5b";
/// 官网登录页：登录后 Cookie 写在 `.hypergryph.com` 下（导入向导的登录窗口打开它）。
pub const LOGIN_URL: &str = "https://ak.hypergryph.com/user/login";
/// 登录后用 Cookie 查询账号 token 的地址（手动获取 token 的说明也指向它）。
pub const TOKEN_URL: &str = "https://web-api.hypergryph.com/account/info/hg";

/// 各接口的域名，测试时全部指向同一个模拟服务器。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HgEndpoints {
    /// 账号服务（grant）。
    pub account: String,
    /// 角色绑定服务。
    pub binding: String,
    /// 明日方舟官网。
    pub arknights: String,
    /// 终末地网页服务。
    pub endfield: String,
    /// 官网账号接口（用 Cookie 查 token）。
    pub web: String,
}

impl Default for HgEndpoints {
    fn default() -> Self {
        Self {
            account: "https://as.hypergryph.com".into(),
            binding: "https://binding-api-account-prod.hypergryph.com".into(),
            arknights: "https://ak.hypergryph.com".into(),
            endfield: "https://ef-webview.hypergryph.com".into(),
            web: "https://web-api.hypergryph.com".into(),
        }
    }
}

impl HgEndpoints {
    /// 全部指向 `base`（测试用）。
    pub fn all(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        Self {
            account: base.clone(),
            binding: base.clone(),
            arknights: base.clone(),
            endfield: base.clone(),
            web: base,
        }
    }
}

/// 账号下绑定的一个游戏角色。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HgRole {
    /// 游戏（`arknights` / `endfield`）。
    pub game: GameId,
    /// 游戏 UID。
    pub uid: String,
    /// 游戏内昵称。
    pub nickname: Option<String>,
    /// 渠道（官服 / B 服）。
    pub channel: Option<String>,
    /// 终末地服务器 ID。
    pub server_id: Option<String>,
}

/// 账号服务的响应：`{ status, msg, data }`。
#[derive(Deserialize)]
struct AccountResp<T> {
    status: i64,
    #[serde(default)]
    msg: String,
    data: Option<T>,
}

/// 游戏网页服务的响应：`{ code, msg, data }`。
#[derive(Deserialize)]
pub(crate) struct GameResp<T> {
    code: i64,
    #[serde(default)]
    msg: String,
    data: Option<T>,
}

impl<T> GameResp<T> {
    /// 业务错误码转成 [`AppError`]：登录失效类提示重新登录，其余按网络错误重试。
    pub(crate) fn into_data(self) -> Result<T> {
        if self.code == 0 {
            return self
                .data
                .ok_or_else(|| AppError::Network("empty response".into()));
        }
        Err(game_error(self.code, &self.msg))
    }
}

fn game_error(code: i64, msg: &str) -> AppError {
    let expired = matches!(code, 3 | 401 | 403 | 10001)
        || ["登录", "token", "Token", "过期", "expired"]
            .iter()
            .any(|k| msg.contains(k));
    if expired {
        AppError::AuthExpired
    } else {
        AppError::Network(format!("code {code}: {msg}"))
    }
}

#[derive(Deserialize)]
struct TokenData {
    token: String,
}

#[derive(Deserialize)]
struct BindingData {
    #[serde(default)]
    list: Vec<BindingApp>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindingApp {
    app_code: String,
    #[serde(default)]
    binding_list: Vec<Binding>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Binding {
    uid: Value,
    #[serde(default)]
    nick_name: Option<String>,
    #[serde(default)]
    channel_name: Option<String>,
    #[serde(default)]
    roles: Vec<BindingRole>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindingRole {
    #[serde(default)]
    server_id: Option<Value>,
    #[serde(default)]
    nick_name: Option<String>,
}

/// 从用户粘贴的内容里取出账号 token：可以是 token 本身，也可以是
/// [`TOKEN_URL`] 返回的整段 JSON（`{"code":0,"data":{"content":"..."}}`）。
pub fn normalize_token(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_matches('"').trim();
    if raw.starts_with('{') {
        let v: Value = serde_json::from_str(raw).ok()?;
        let content = v
            .pointer("/data/content")
            .or_else(|| v.get("content"))
            .or_else(|| v.get("token"))?;
        return content.as_str().and_then(normalize_token);
    }
    let ok = (16..=1024).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=_-.".contains(&b));
    ok.then(|| raw.to_owned())
}

/// 鹰角接口客户端：账号授权与角色查询。两款游戏的拉取器共用。
#[derive(Clone)]
pub struct HgClient {
    pub(crate) client: reqwest::Client,
    pub(crate) pacing: Pacing,
    pub(crate) endpoints: Arc<HgEndpoints>,
}

impl HgClient {
    /// 指定 HTTP 客户端（代理设置）与请求节奏。
    pub fn new(client: reqwest::Client, pacing: Pacing) -> Self {
        Self {
            client,
            pacing,
            endpoints: Arc::new(HgEndpoints::default()),
        }
    }

    /// 改用指定的接口地址（测试用）。
    pub fn with_endpoints(mut self, endpoints: HgEndpoints) -> Self {
        self.endpoints = Arc::new(endpoints);
        self
    }

    /// 发请求并解析账号服务的响应，网络错误与限流时重试。
    async fn account_call<T: DeserializeOwned>(
        &self,
        cancel: &CancelToken,
        build: impl Fn() -> reqwest::RequestBuilder,
        expired_on_error: bool,
    ) -> Result<T> {
        with_retry(&self.pacing, cancel, || {
            let req = build();
            async move {
                let resp = req.send().await.map_err(network_error)?;
                let body: AccountResp<T> =
                    check_status(resp)?.json().await.map_err(network_error)?;
                match (body.status, body.data) {
                    (0, Some(d)) => Ok(d),
                    (0, None) => Err(AppError::Network("empty response".into())),
                    (code, _) if expired_on_error || code == 3 => {
                        tracing::debug!(code, "鹰角账号接口拒绝了 token");
                        Err(AppError::AuthExpired)
                    }
                    (code, _) => Err(AppError::Network(format!("status {code}: {}", body.msg))),
                }
            }
        })
        .await
    }

    /// 账号 token → OAuth token。token 无效或过期时为 [`AppError::AuthExpired`]。
    async fn grant(&self, token: &str, cancel: &CancelToken) -> Result<String> {
        let url = format!("{}/user/oauth2/v2/grant", self.endpoints.account);
        let body = serde_json::json!({ "token": token, "appCode": APP_CODE, "type": 1 });
        let d: TokenData = self
            .account_call(cancel, || self.client.post(&url).json(&body), true)
            .await?;
        Ok(d.token)
    }

    /// 账号下绑定的明日方舟与终末地角色。
    pub async fn roles(&self, token: &str, cancel: &CancelToken) -> Result<Vec<HgRole>> {
        let oauth = self.grant(token, cancel).await?;
        self.roles_with(&oauth, cancel).await
    }

    async fn roles_with(&self, oauth: &str, cancel: &CancelToken) -> Result<Vec<HgRole>> {
        let url = format!("{}/account/binding/v1/binding_list", self.endpoints.binding);
        let d: BindingData = self
            .account_call(
                cancel,
                || self.client.get(&url).query(&[("token", oauth)]),
                false,
            )
            .await?;
        let mut out = Vec::new();
        for app in d.list {
            let game: GameId = match app.app_code.as_str() {
                "arknights" => GameId::ARKNIGHTS.into(),
                "endfield" => GameId::ENDFIELD.into(),
                _ => continue,
            };
            for b in app.binding_list {
                let Some(uid) = json_text(&b.uid).filter(|u| !u.is_empty()) else {
                    continue;
                };
                let role = b.roles.first();
                out.push(HgRole {
                    game: game.clone(),
                    uid,
                    nickname: b
                        .nick_name
                        .clone()
                        .or_else(|| role.and_then(|r| r.nick_name.clone())),
                    channel: b.channel_name.clone(),
                    server_id: role.and_then(|r| r.server_id.as_ref().and_then(json_text)),
                });
            }
        }
        Ok(out)
    }

    /// 按 UID 换取角色凭证（U8 token）。
    pub(crate) async fn u8_token(
        &self,
        oauth: &str,
        uid: &str,
        cancel: &CancelToken,
    ) -> Result<String> {
        let url = format!(
            "{}/account/binding/v1/u8_token_by_uid",
            self.endpoints.binding
        );
        let body = serde_json::json!({ "token": oauth, "uid": uid });
        let d: TokenData = self
            .account_call(cancel, || self.client.post(&url).json(&body), false)
            .await?;
        Ok(d.token)
    }

    /// 用登录窗口里的 Cookie 查询账号 token（`Cookie` 请求头格式）。未登录时为空。
    pub async fn token_from_cookies(&self, cookie: &str) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Content {
            content: String,
        }
        let url = format!("{}/account/info/hg", self.endpoints.web);
        let resp = self
            .client
            .get(url)
            .header(reqwest::header::COOKIE, cookie)
            .send()
            .await
            .map_err(network_error)?;
        let body: GameResp<Content> = check_status(resp)?.json().await.map_err(network_error)?;
        Ok(match body.into_data() {
            Ok(c) => normalize_token(&c.content),
            Err(AppError::AuthExpired) => None,
            Err(AppError::Network(_)) => None,
            Err(e) => return Err(e),
        })
    }
}

/// 鹰角两款游戏之一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HgGame {
    /// 明日方舟。
    Arknights,
    /// 明日方舟：终末地。
    Endfield,
}

impl HgGame {
    /// 游戏 ID。
    pub fn id(self) -> GameId {
        match self {
            Self::Arknights => GameId::ARKNIGHTS.into(),
            Self::Endfield => GameId::ENDFIELD.into(),
        }
    }
}

/// 鹰角 token 拉取器：拉取 `FetchSource::uid` 指定的一个角色。
pub struct HgFetcher {
    game: HgGame,
    hg: HgClient,
}

impl HgFetcher {
    /// 创建。
    pub fn new(game: HgGame, hg: HgClient) -> Self {
        Self { game, hg }
    }
}

#[async_trait]
impl Fetcher for HgFetcher {
    fn kind(&self) -> FetchKind {
        FetchKind::AccountToken
    }

    /// token 拉取没有可自动查找的链接；已保存的 token 由外壳从凭证表里取。
    async fn discover(&self, _env: &dyn PathProvider) -> Result<Vec<FetchSource>> {
        Ok(Vec::new())
    }

    async fn fetch(
        &self,
        src: &FetchSource,
        since: &dyn SinceLookup,
        progress: &dyn Progress,
        cancel: &CancelToken,
    ) -> Result<Vec<ImportedAccount>> {
        let token = src
            .token
            .as_ref()
            .ok_or_else(|| AppError::Invalid("missing account token".into()))?;
        let uid = src
            .uid
            .as_deref()
            .ok_or_else(|| AppError::Invalid("missing uid".into()))?;
        let oauth = self.hg.grant(token.expose(), cancel).await?;
        let roles = self.hg.roles_with(&oauth, cancel).await?;
        let game = self.game.id();
        let role = roles
            .into_iter()
            .find(|r| r.game == game && r.uid == uid)
            .ok_or_else(|| {
                AppError::NotFound(format!("{game} role {} not bound", Redacted::uid(uid)))
            })?;
        let role_token = self.hg.u8_token(&oauth, uid, cancel).await?;
        let since = since.since(&game, uid)?;
        let mut acc = ImportedAccount::new(game, uid, gachanote_core::model::DEFAULT_TZ_OFFSET_MIN);
        acc.region = Some("cn".into());
        acc.nickname = role.nickname.clone();
        acc.lang = Some("zh-cn".into());
        let ctx = Ctx {
            hg: &self.hg,
            since: &since,
            progress,
            cancel,
        };
        match self.game {
            HgGame::Arknights => {
                arknights::fetch(&ctx, token.expose(), &role_token, uid, &mut acc).await?
            }
            HgGame::Endfield => endfield::fetch(&ctx, &role_token, &role, &mut acc).await?,
        }
        Ok(vec![acc])
    }
}

/// 一次拉取的上下文。
pub(crate) struct Ctx<'a> {
    pub hg: &'a HgClient,
    pub since: &'a BTreeMap<gachanote_core::PoolTypeId, gachanote_core::model::Cursor>,
    pub progress: &'a dyn Progress,
    pub cancel: &'a CancelToken,
}

impl Ctx<'_> {
    /// 发一个 GET 请求并解析游戏服务的响应，网络错误与限流时重试。
    pub(crate) async fn get<T: DeserializeOwned>(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<T> {
        with_retry(&self.hg.pacing, self.cancel, || {
            let req = build();
            async move {
                let resp = req.send().await.map_err(network_error)?;
                let body: GameResp<T> = check_status(resp)?.json().await.map_err(network_error)?;
                body.into_data()
            }
        })
        .await
    }

    /// 相邻请求之间的等待。
    pub(crate) async fn pause(&self) {
        tokio::time::sleep(self.hg.pacing.delay()).await;
    }
}

/// 字符串或数字字段的毫秒时间戳。
pub(crate) fn ts(v: Option<&Value>) -> Option<i64> {
    v.and_then(json_text)?.trim().parse().ok()
}

#[cfg(test)]
mod tests;
