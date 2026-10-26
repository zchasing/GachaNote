//! 自动备份与恢复（架构文档 4.2，FR-IMP-06）。
//!
//! 用 SQLite Online Backup 复制到 `backups/gachanote-YYYYMMDD-HHMMSS.db`（本地时间），
//! 同一秒内多次备份时加 `-1`、`-2` 后缀；只保留最近 [`DEFAULT_KEEP`] 份。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gachanote_core::import::now_ms;
use gachanote_core::repo::{BackupInfo, BackupReason, BackupService, RecordRepository};
use gachanote_core::{AppError, Result};

use crate::sqlite::SqliteStore;

/// 默认保留份数。
pub const DEFAULT_KEEP: usize = 5;

const PREFIX: &str = "gachanote-";
const SUFFIX: &str = ".db";

/// 从文件名解析排序键 `(时间戳, 序号)`；不是本软件的备份文件时返回 `None`。
fn parse_name(name: &str) -> Option<(String, u32)> {
    let body = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let (stamp, n) = match body.get(15..) {
        Some("") => (body, 0),
        Some(rest) => (&body[..15], rest.strip_prefix('-')?.parse().ok()?),
        None => return None,
    };
    let valid = stamp.len() == 15
        && stamp
            .char_indices()
            .all(|(i, c)| if i == 8 { c == '-' } else { c.is_ascii_digit() });
    valid.then(|| (stamp.to_owned(), n))
}

/// 备份目录管理。
pub struct BackupManager {
    dir: PathBuf,
    keep: usize,
    store: Arc<SqliteStore>,
}

impl BackupManager {
    /// 备份到 `dir`，保留最近 `keep` 份（至少 1 份）。
    pub fn new(dir: impl Into<PathBuf>, keep: usize, store: Arc<SqliteStore>) -> Self {
        Self {
            dir: dir.into(),
            keep: keep.max(1),
            store,
        }
    }

    /// 备份目录。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 把 `store` 备份进 `dir` 并清理旧备份。用于还没有 `BackupManager` 的场合（如迁移前）。
    pub fn snapshot_into(
        dir: &Path,
        keep: usize,
        store: &dyn RecordRepository,
        reason: BackupReason,
    ) -> Result<BackupInfo> {
        std::fs::create_dir_all(dir)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let mut n = 0u32;
        let path = loop {
            let name = if n == 0 {
                format!("{PREFIX}{stamp}{SUFFIX}")
            } else {
                format!("{PREFIX}{stamp}-{n}{SUFFIX}")
            };
            let path = dir.join(name);
            if !path.exists() {
                break path;
            }
            n += 1;
        };
        store.backup(&path)?;
        tracing::info!(?reason, file = %path.display(), "已备份数据库");
        prune(dir, keep.max(1))?;
        info(&path)
    }

    /// 全部备份，最新的在前。
    pub fn list(&self) -> Result<Vec<BackupInfo>> {
        let mut found = scan(&self.dir)?;
        found.reverse();
        found.into_iter().map(|(_, p)| info(&p)).collect()
    }

    /// 用某份备份替换当前数据库。
    pub fn restore(&self, name: &str) -> Result<()> {
        if parse_name(name).is_none() {
            return Err(AppError::Invalid(format!("not a backup file: {name}")));
        }
        self.store.restore_from(&self.dir.join(name))?;
        tracing::info!(name, "已从备份恢复");
        Ok(())
    }
}

impl BackupService for BackupManager {
    fn snapshot(&self, reason: BackupReason) -> Result<BackupInfo> {
        Self::snapshot_into(&self.dir, self.keep, self.store.as_ref(), reason)
    }
}

/// 目录中的备份，按时间从旧到新。
fn scan(dir: &Path) -> Result<Vec<((String, u32), PathBuf)>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if let Some(key) = name.to_str().and_then(parse_name) {
            found.push((key, entry.path()));
        }
    }
    found.sort();
    Ok(found)
}

fn prune(dir: &Path, keep: usize) -> Result<()> {
    let found = scan(dir)?;
    let excess = found.len().saturating_sub(keep);
    for (_, path) in found.into_iter().take(excess) {
        std::fs::remove_file(&path)?;
        tracing::debug!(file = %path.display(), "删除旧备份");
    }
    Ok(())
}

fn info(path: &Path) -> Result<BackupInfo> {
    let meta = std::fs::metadata(path)?;
    let created_at = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or_else(now_ms);
    Ok(BackupInfo {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: path.to_path_buf(),
        created_at,
        size: meta.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_backup_names() {
        assert_eq!(
            parse_name("gachanote-20261025-120000.db"),
            Some(("20261025-120000".into(), 0))
        );
        assert_eq!(
            parse_name("gachanote-20261025-120000-12.db"),
            Some(("20261025-120000".into(), 12))
        );
        for bad in [
            "gachanote.db",
            "gachanote-2026.db",
            "gachanote-20261025x120000.db",
            "gachanote-20261025-12000a.db",
            "gachanote-20261025-120000_1.db",
            "gachanote-20261025-120000-x.db",
            "other-20261025-120000.db",
            "../gachanote-20261025-120000.db",
        ] {
            assert_eq!(parse_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn same_second_suffix_sorts_after_base() {
        let mut v = [
            parse_name("gachanote-20261025-120000-2.db").unwrap(),
            parse_name("gachanote-20261025-120000.db").unwrap(),
            parse_name("gachanote-20261025-115959-9.db").unwrap(),
            parse_name("gachanote-20261025-120000-1.db").unwrap(),
        ];
        v.sort();
        let n: Vec<_> = v.iter().map(|(s, n)| format!("{s}/{n}")).collect();
        assert_eq!(
            n,
            [
                "20261025-115959/9",
                "20261025-120000/0",
                "20261025-120000/1",
                "20261025-120000/2"
            ]
        );
    }
}
