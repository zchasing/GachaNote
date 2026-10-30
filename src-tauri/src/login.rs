//! 鹰角网络通行证登录窗口（T-52，FR-FET-08）：内嵌官网登录页，登录后用窗口里的
//! Cookie 查询账号 token，查到后自动关窗。用户只需要正常登录，不必手动复制 token。
//!
//! 窗口是隐身模式（不留登录状态），且没有任何 capability，网页调用不了应用命令。
//! 官网改版导致取不到 token 时，用户仍可在导入向导里手动粘贴。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gachanote_core::{AppError, Result};
use gachanote_games::hypergryph::{LOGIN_URL, TOKEN_URL};
use gachanote_games::HgClient;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// 登录窗口的标签。
pub const WINDOW: &str = "hg-login";
/// 两次查询 Cookie 的间隔。
const POLL: Duration = Duration::from_millis(1500);
/// 超过这个时间仍未登录，关窗并报错。
const TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(e.to_string())
}

/// 在官网页面顶部固定一行提示（UI/UX 文档 4.4）。只操作页面 DOM，不涉及应用命令。
fn banner_script(hint: &str) -> String {
    let text = serde_json::to_string(hint).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(function () {{
  if (window.top !== window) return;
  function add() {{
    if (document.getElementById('gachanote-hint')) return;
    var d = document.createElement('div');
    d.id = 'gachanote-hint';
    d.textContent = {text};
    d.style.cssText = 'position:fixed;top:0;left:0;right:0;z-index:2147483647;padding:6px 12px;' +
      'background:#3b6fe0;color:#fff;font:13px/1.5 system-ui,sans-serif;text-align:center;pointer-events:none';
    document.documentElement.appendChild(d);
    document.documentElement.style.paddingTop = '32px';
  }}
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', add);
  else add();
}})();"#
    )
}

/// 打开登录窗口并等待 token；用户关窗时为 [`AppError::Cancelled`]。
///
/// 必须在异步命令里调用：Windows 上在主线程读取 Cookie 会死锁。
pub async fn hg_login(app: &AppHandle, title: &str, hint: &str, hg: &HgClient) -> Result<String> {
    if let Some(old) = app.get_webview_window(WINDOW) {
        let _ = old.close();
    }
    let login = LOGIN_URL.parse().map_err(internal)?;
    let token_url: tauri::Url = TOKEN_URL.parse().map_err(internal)?;
    let window = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::External(login))
        .title(title)
        .inner_size(480.0, 760.0)
        .center()
        .incognito(true)
        .initialization_script(banner_script(hint))
        .build()
        .map_err(internal)?;
    let closed = Arc::new(AtomicBool::new(false));
    {
        let closed = Arc::clone(&closed);
        window.on_window_event(move |e| {
            if matches!(e, WindowEvent::Destroyed) {
                closed.store(true, Ordering::SeqCst);
            }
        });
    }
    let deadline = Instant::now() + TIMEOUT;
    loop {
        tokio::time::sleep(POLL).await;
        if closed.load(Ordering::SeqCst) {
            return Err(AppError::Cancelled);
        }
        if Instant::now() > deadline {
            let _ = window.close();
            return Err(AppError::Network("login timed out".into()));
        }
        let cookies = match window.cookies_for_url(token_url.clone()) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "读取登录窗口 Cookie 失败");
                continue;
            }
        };
        if cookies.is_empty() {
            continue;
        }
        let header = cookies
            .iter()
            .map(|c| format!("{}={}", c.name(), c.value()))
            .collect::<Vec<_>>()
            .join("; ");
        match hg.token_from_cookies(&header).await {
            Ok(Some(token)) => {
                tracing::info!("已从登录窗口取得鹰角 token");
                let _ = window.close();
                return Ok(token);
            }
            Ok(None) => {}
            Err(e) => tracing::debug!(error = %e, "查询鹰角 token 失败"),
        }
    }
}
