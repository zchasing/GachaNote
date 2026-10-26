//! 网络请求的公共部分：请求间隔、重试与退避（SRS FR-FET-05）。

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gachanote_core::task::CancelToken;
use gachanote_core::{AppError, Result};

/// 请求节奏：相邻请求的间隔与抖动、失败重试次数与退避。
#[derive(Debug, Clone)]
pub struct Pacing {
    /// 相邻两次请求的最小间隔。
    pub interval: Duration,
    /// 在间隔之上随机增加的最大时长。
    pub jitter: Duration,
    /// 网络错误或限流时最多重试几次。
    pub retries: u32,
    /// 第一次重试前的等待，之后每次翻倍。
    pub backoff: Duration,
}

impl Default for Pacing {
    /// 间隔 300–500 ms，最多重试 3 次，退避 1 s / 2 s / 4 s（架构文档 7.2）。
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(300),
            jitter: Duration::from_millis(200),
            retries: 3,
            backoff: Duration::from_secs(1),
        }
    }
}

impl Pacing {
    /// 不等待（测试用），仍会重试。
    pub fn immediate() -> Self {
        Self {
            interval: Duration::ZERO,
            jitter: Duration::ZERO,
            retries: 3,
            backoff: Duration::ZERO,
        }
    }

    /// 下一次请求前的等待时长。
    pub(crate) fn delay(&self) -> Duration {
        let jitter_ms = u64::try_from(self.jitter.as_millis()).unwrap_or(u64::MAX);
        if jitter_ms == 0 {
            return self.interval;
        }
        // 不必是密码学随机，取当前时间的纳秒部分即可
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::from(d.subsec_nanos()))
            .unwrap_or(0);
        self.interval + Duration::from_millis(nanos % (jitter_ms + 1))
    }

    /// 第 `attempt` 次重试（从 0 开始）前的等待时长。
    pub(crate) fn backoff(&self, attempt: u32) -> Duration {
        self.backoff.saturating_mul(1 << attempt.min(16))
    }
}

/// 默认 HTTP 客户端：15 秒超时，跟随系统代理。
pub fn default_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("GachaNote/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

/// 把 reqwest 错误转成 [`AppError::Network`]。错误信息里的 URL（含 authkey）一律去掉。
pub(crate) fn network_error(e: reqwest::Error) -> AppError {
    AppError::Network(e.without_url().to_string())
}

/// 检查 HTTP 状态码：429 视为限流，其他非 2xx 视为网络错误。
pub(crate) fn check_status(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        Err(AppError::RateLimited)
    } else if !status.is_success() {
        Err(AppError::Network(format!("http status {status}")))
    } else {
        Ok(resp)
    }
}

/// 执行请求：网络错误与限流按指数退避重试，其他错误（如链接过期）直接返回。
pub(crate) async fn with_retry<T, F, Fut>(
    pacing: &Pacing,
    cancel: &CancelToken,
    mut request: F,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 0;
    loop {
        cancel.check()?;
        match request().await {
            Err(AppError::Network(_) | AppError::RateLimited) if attempt < pacing.retries => {
                tracing::debug!(attempt, "请求失败，退避后重试");
                tokio::time::sleep(pacing.backoff(attempt)).await;
                attempt += 1;
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn delays() {
        let p = Pacing::default();
        for _ in 0..20 {
            let d = p.delay();
            assert!(d >= Duration::from_millis(300) && d <= Duration::from_millis(500));
        }
        assert_eq!(p.backoff(0), Duration::from_secs(1));
        assert_eq!(p.backoff(2), Duration::from_secs(4));
        assert_eq!(Pacing::immediate().delay(), Duration::ZERO);
    }

    #[tokio::test]
    async fn retries_transient_errors_only() {
        let pacing = Pacing::immediate();
        let cancel = CancelToken::new();
        let calls = AtomicU32::new(0);
        let r: Result<u32> = with_retry(&pacing, &cancel, || async {
            match calls.fetch_add(1, Ordering::SeqCst) {
                0 => Err(AppError::RateLimited),
                1 => Err(AppError::Network("reset".into())),
                n => Ok(n),
            }
        })
        .await;
        assert_eq!(r, Ok(2));

        calls.store(0, Ordering::SeqCst);
        let r: Result<u32> = with_retry(&pacing, &cancel, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(AppError::RateLimited)
        })
        .await;
        assert_eq!(r, Err(AppError::RateLimited));
        assert_eq!(calls.load(Ordering::SeqCst), 4);

        let r: Result<u32> =
            with_retry(&pacing, &cancel, || async { Err(AppError::AuthExpired) }).await;
        assert_eq!(r, Err(AppError::AuthExpired));

        cancel.cancel();
        let r: Result<u32> = with_retry(&pacing, &cancel, || async { Ok(1) }).await;
        assert_eq!(r, Err(AppError::Cancelled));
    }
}
