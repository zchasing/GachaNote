//! 软件自动更新（T-57，F-24）：检查 GitHub Releases 上的 `latest.json`，
//! 用户确认后下载、校验签名、安装并重启。
//!
//! 签名公钥写在 `tauri.conf.json` 的 `plugins.updater.pubkey`（发布时可由工作流注入）；
//! 为空时视为未配置，检查更新直接报错，界面引导用户去发布页手动下载。

use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use gachanote_core::{AppError, Result};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event;

use crate::dto::{UpdateInfo, UpdateProgress};

/// 检查到、等待用户确认安装的更新。
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// 两次进度事件之间至少下载这么多字节。
const PROGRESS_STEP: u64 = 256 * 1024;

fn network(e: impl std::fmt::Display) -> AppError {
    AppError::Network(e.to_string())
}

/// 是否配置了签名公钥。
pub fn configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|v| v.get("pubkey"))
        .and_then(|v| v.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

/// 检查新版本；有新版本时记下，等待 [`install`]。
pub async fn check(app: &AppHandle, proxy: Option<&str>) -> Result<Option<UpdateInfo>> {
    if !configured(app) {
        return Err(AppError::Invalid("updater is not configured".into()));
    }
    let mut builder = app.updater_builder().timeout(Duration::from_secs(20));
    if let Some(p) = proxy
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .and_then(|p| p.parse().ok())
    {
        builder = builder.proxy(p);
    }
    let update = builder
        .build()
        .map_err(network)?
        .check()
        .await
        .map_err(network)?;
    let info = update.as_ref().map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        date: u
            .raw_json
            .get("pub_date")
            .and_then(|v| v.as_str())
            .map(String::from),
        notes: u.body.clone().filter(|b| !b.trim().is_empty()),
    });
    *app.state::<PendingUpdate>()
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = update;
    Ok(info)
}

/// 下载并安装上次检查到的更新，发 `update://progress`，完成后重启。
pub async fn install(app: &AppHandle) -> Result<()> {
    let update = app
        .state::<PendingUpdate>()
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .ok_or_else(|| AppError::NotFound("update".into()))?;
    tracing::info!(version = %update.version, "开始下载更新");
    let mut downloaded = 0u64;
    let mut reported = 0u64;
    let handle = app.clone();
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                if downloaded - reported >= PROGRESS_STEP || Some(downloaded) == total {
                    reported = downloaded;
                    let _ = UpdateProgress { downloaded, total }.emit(&handle);
                }
            },
            || tracing::info!("更新下载完成，开始安装"),
        )
        .await
        .map_err(network)?;
    app.restart()
}
