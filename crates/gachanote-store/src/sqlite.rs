//! SQLite 实现。

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use gachanote_core::import::{now_ms, WRITE_CHUNK};
use gachanote_core::model::{
    Account, AccountImport, AccountInsert, Cursor, GachaRecord, ImportMeta, InsertSummary,
    NewAccount, NewRecord, Page, RecordQuery, SortOrder,
};
use gachanote_core::repo::{BackupReason, BackupService, RecordRepository};
use gachanote_core::task::CancelToken;
use gachanote_core::{AccountId, AppError, GameId, PoolTypeId, Result};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use rusqlite_migration::{Migrations, M};

/// 迁移脚本，按顺序追加，不得修改已发布的脚本。
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql")];

/// 当前数据库结构版本（`PRAGMA user_version`）。
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

fn migrations() -> Migrations<'static> {
    Migrations::new(MIGRATIONS.iter().map(|sql| M::up(sql)).collect())
}

pub(crate) fn storage(e: impl std::fmt::Display) -> AppError {
    AppError::Storage(e.to_string())
}

fn user_version(conn: &Connection) -> Result<usize> {
    let v: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(storage)?;
    usize::try_from(v).map_err(storage)
}

fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// 迁移前的备份回调：接收一个临时打开的、尚未迁移的数据库。
pub type MigrationBackup<'a> = dyn Fn(&SqliteStore) -> Result<()> + 'a;

/// 本地 SQLite 仓库。
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

const RECORD_COLUMNS: &str = "id, account_id, pool_type, banner_id, ext_id, item_id, item_name, \
     item_type, rarity, pulled_at, seq_in_sec, is_pull, batch_id, raw_json";

fn record_from_row(row: &Row<'_>) -> rusqlite::Result<GachaRecord> {
    Ok(GachaRecord {
        id: row.get::<_, String>(0)?.into(),
        account_id: row.get::<_, String>(1)?.into(),
        data: NewRecord {
            pool_type: row.get::<_, String>(2)?.into(),
            banner_id: row.get(3)?,
            ext_id: row.get(4)?,
            item_id: row.get(5)?,
            item_name: row.get(6)?,
            item_type: row.get(7)?,
            rarity: row.get(8)?,
            pulled_at: row.get(9)?,
            seq_in_sec: row.get(10)?,
            is_pull: row.get(11)?,
            raw_json: row.get(13)?,
        },
        batch_id: row.get::<_, Option<String>>(12)?.map(Into::into),
    })
}

const ACCOUNT_COLUMNS: &str = "id, game, uid, region, nickname, tz_offset_min, created_at";

fn account_from_row(row: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get::<_, String>(0)?.into(),
        game: row.get::<_, String>(1)?.into(),
        uid: row.get(2)?,
        region: row.get(3)?,
        nickname: row.get(4)?,
        tz_offset_min: row.get(5)?,
        created_at: row.get(6)?,
    })
}

/// 在事务内新建或补全账号，返回账号与是否新建。
fn upsert_account_tx(tx: &Connection, a: &NewAccount) -> rusqlite::Result<(Account, bool)> {
    let existing = tx
        .query_row(
            &format!("SELECT {ACCOUNT_COLUMNS} FROM account WHERE game = ?1 AND uid = ?2"),
            params![a.game.as_str(), a.uid],
            account_from_row,
        )
        .optional()?;
    match existing {
        Some(acc) => {
            tx.execute(
                "UPDATE account SET region = COALESCE(region, ?2), nickname = COALESCE(nickname, ?3) WHERE id = ?1",
                params![acc.id.as_str(), a.region, a.nickname],
            )?;
            let acc = tx.query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM account WHERE id = ?1"),
                params![acc.id.as_str()],
                account_from_row,
            )?;
            Ok((acc, false))
        }
        None => {
            let acc = Account {
                id: new_id().into(),
                game: a.game.clone(),
                uid: a.uid.clone(),
                region: a.region.clone(),
                nickname: a.nickname.clone(),
                tz_offset_min: a.tz_offset_min,
                created_at: now_ms(),
            };
            tx.execute(
                &format!(
                    "INSERT INTO account ({ACCOUNT_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                ),
                params![
                    acc.id.as_str(),
                    acc.game.as_str(),
                    acc.uid,
                    acc.region,
                    acc.nickname,
                    acc.tz_offset_min,
                    acc.created_at
                ],
            )?;
            Ok((acc, true))
        }
    }
}

fn insert_all(
    tx: &Transaction<'_>,
    meta: &ImportMeta,
    data: &[AccountImport],
    cancel: &CancelToken,
) -> Result<InsertSummary> {
    let mut summary = InsertSummary::default();
    let mut insert = tx
        .prepare(&format!(
            "INSERT INTO gacha_record ({RECORD_COLUMNS}) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) ON CONFLICT DO NOTHING"
        ))
        .map_err(storage)?;
    for imp in data {
        cancel.check()?;
        let (account, created) = upsert_account_tx(tx, &imp.account).map_err(storage)?;
        let batch_id = new_id();
        tx.execute(
            "INSERT INTO import_batch (id, account_id, source, format, file_name, added, skipped, started_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6)",
            params![
                batch_id,
                account.id.as_str(),
                meta.source.as_str(),
                meta.format.map(|f| f.as_str()),
                meta.file_name,
                meta.started_at
            ],
        )
        .map_err(storage)?;
        let (mut added, mut skipped) = (0u32, 0u32);
        for (i, r) in imp.records.iter().enumerate() {
            if i % WRITE_CHUNK == 0 {
                cancel.check()?;
            }
            let n = insert
                .execute(params![
                    new_id(),
                    account.id.as_str(),
                    r.pool_type.as_str(),
                    r.banner_id,
                    r.ext_id,
                    r.item_id,
                    r.item_name,
                    r.item_type,
                    r.rarity,
                    r.pulled_at,
                    r.seq_in_sec,
                    r.is_pull,
                    batch_id,
                    r.raw_json
                ])
                .map_err(storage)?;
            if n == 1 {
                added += 1;
            } else {
                skipped += 1;
            }
        }
        tx.execute(
            "UPDATE import_batch SET added = ?2, skipped = ?3, finished_at = ?4 WHERE id = ?1",
            params![batch_id, added, skipped, now_ms()],
        )
        .map_err(storage)?;
        summary.accounts.push(AccountInsert {
            account,
            created,
            batch_id: batch_id.into(),
            added,
            skipped,
        });
    }
    cancel.check()?;
    Ok(summary)
}

impl SqliteStore {
    fn configure(conn: &Connection) -> Result<()> {
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(storage)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage)?;
        Ok(())
    }

    fn from_connection(mut conn: Connection) -> Result<Self> {
        Self::configure(&conn)?;
        migrations().to_latest(&mut conn).map_err(storage)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 打开（不存在时创建）数据库文件，开启 WAL 并迁移到最新版本。
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_backup(path, None)
    }

    /// 同 [`SqliteStore::open`]；已有数据且需要升级结构时，先用 `backup` 做一份备份
    /// （NFR-09）。`backup` 接收一个临时打开的、尚未迁移的数据库。
    pub fn open_with_backup(
        path: &Path,
        backup: Option<&MigrationBackup<'_>>,
    ) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).map_err(storage)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
        let version = user_version(&conn)?;
        if version > SCHEMA_VERSION {
            return Err(AppError::Storage(format!(
                "database schema {version} is newer than supported {SCHEMA_VERSION}"
            )));
        }
        if version > 0 && version < SCHEMA_VERSION {
            if let Some(backup) = backup {
                tracing::info!(from = version, to = SCHEMA_VERSION, "升级数据库结构前备份");
                backup(&Self {
                    conn: Mutex::new(conn),
                })?;
                return Self::from_connection(Connection::open(path).map_err(storage)?);
            }
        }
        Self::from_connection(conn)
    }

    /// 内存数据库（测试用）。
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(storage)?)
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 当前结构版本。
    pub fn schema_version(&self) -> Result<usize> {
        user_version(&self.conn())
    }

    /// 用备份文件的内容整体替换当前数据库（设置页「从备份恢复」）。
    pub fn restore_from(&self, src: &Path) -> Result<()> {
        if !src.is_file() {
            return Err(AppError::NotFound(src.display().to_string()));
        }
        let mut conn = self.conn();
        conn.restore(
            rusqlite::MAIN_DB,
            src,
            None::<fn(rusqlite::backup::Progress)>,
        )
        .map_err(storage)?;
        Self::configure(&conn)?;
        // 旧版本的备份恢复后同样升级到最新结构
        migrations().to_latest(&mut conn).map_err(storage)?;
        Ok(())
    }

    /// 删除账号，删除前先做一份备份。
    pub fn delete_account_with_backup(
        &self,
        id: &AccountId,
        backup: &dyn BackupService,
    ) -> Result<()> {
        if self.account(id)?.is_none() {
            return Err(AppError::NotFound(id.to_string()));
        }
        backup.snapshot(BackupReason::DeleteAccount)?;
        self.delete_account(id)
    }

    /// 读取设置（JSON）。
    pub fn setting(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let raw: Option<String> = self
            .conn()
            .query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?;
        raw.map(|s| serde_json::from_str(&s).map_err(storage))
            .transpose()
    }

    /// 写入设置（JSON）。
    pub fn set_setting(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        self.conn()
            .execute(
                "INSERT INTO setting (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value.to_string()],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// 某账号的导入批次数（测试与诊断用）。
    pub fn batch_count(&self, account: &AccountId) -> Result<u64> {
        let n: i64 = self
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM import_batch WHERE account_id = ?1",
                [account.as_str()],
                |r| r.get(0),
            )
            .map_err(storage)?;
        Ok(u64::try_from(n).unwrap_or(0))
    }
}

impl RecordRepository for SqliteStore {
    fn accounts(&self, game: Option<&GameId>) -> Result<Vec<Account>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {ACCOUNT_COLUMNS} FROM account WHERE ?1 IS NULL OR game = ?1 ORDER BY created_at, id"
            ))
            .map_err(storage)?;
        let rows = stmt
            .query_map([game.map(GameId::as_str)], account_from_row)
            .map_err(storage)?;
        rows.collect::<rusqlite::Result<_>>().map_err(storage)
    }

    fn account(&self, id: &AccountId) -> Result<Option<Account>> {
        self.conn()
            .query_row(
                &format!("SELECT {ACCOUNT_COLUMNS} FROM account WHERE id = ?1"),
                [id.as_str()],
                account_from_row,
            )
            .optional()
            .map_err(storage)
    }

    fn upsert_account(&self, account: &NewAccount) -> Result<Account> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(storage)?;
        let (acc, _) = upsert_account_tx(&tx, account).map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(acc)
    }

    fn rename_account(&self, id: &AccountId, nickname: Option<&str>) -> Result<()> {
        let n = self
            .conn()
            .execute(
                "UPDATE account SET nickname = ?2 WHERE id = ?1",
                params![id.as_str(), nickname],
            )
            .map_err(storage)?;
        if n == 0 {
            return Err(AppError::NotFound(id.to_string()));
        }
        Ok(())
    }

    fn delete_account(&self, id: &AccountId) -> Result<()> {
        let n = self
            .conn()
            .execute("DELETE FROM account WHERE id = ?1", [id.as_str()])
            .map_err(storage)?;
        if n == 0 {
            return Err(AppError::NotFound(id.to_string()));
        }
        Ok(())
    }

    fn insert_records(
        &self,
        meta: &ImportMeta,
        data: &[AccountImport],
        cancel: &CancelToken,
    ) -> Result<InsertSummary> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(storage)?;
        // 出错时 tx 被丢弃，自动回滚
        let summary = insert_all(&tx, meta, data, cancel)?;
        tx.commit().map_err(storage)?;
        Ok(summary)
    }

    fn records(&self, q: &RecordQuery) -> Result<Page<GachaRecord>> {
        let mut filter = String::from("account_id = ?");
        let mut args: Vec<Value> = vec![q.account_id.to_string().into()];
        if !q.pool_types.is_empty() {
            filter.push_str(&format!(
                " AND pool_type IN ({})",
                vec!["?"; q.pool_types.len()].join(",")
            ));
            args.extend(q.pool_types.iter().map(|p| Value::from(p.to_string())));
        }
        if !q.rarities.is_empty() {
            filter.push_str(&format!(
                " AND rarity IN ({})",
                vec!["?"; q.rarities.len()].join(",")
            ));
            args.extend(q.rarities.iter().map(|r| Value::from(i64::from(*r))));
        }
        if let Some(from) = q.from {
            filter.push_str(" AND pulled_at >= ?");
            args.push(from.into());
        }
        if let Some(to) = q.to {
            filter.push_str(" AND pulled_at < ?");
            args.push(to.into());
        }
        if let Some(name) = q.name.as_deref().filter(|n| !n.is_empty()) {
            let escaped = name
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            filter.push_str(" AND item_name LIKE ? ESCAPE '\\'");
            args.push(format!("%{escaped}%").into());
        }
        let dir = match q.order {
            SortOrder::NewestFirst => "DESC",
            SortOrder::OldestFirst => "ASC",
        };
        let conn = self.conn();
        let total: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM gacha_record WHERE {filter}"),
                params_from_iter(args.iter()),
                |r| r.get(0),
            )
            .map_err(storage)?;
        let total = u64::try_from(total).unwrap_or(0);
        let size = q.page_size.clamp(1, 1000);
        args.push(i64::from(size).into());
        args.push((i64::from(q.page) * i64::from(size)).into());
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {RECORD_COLUMNS} FROM gacha_record WHERE {filter} \
                 ORDER BY pulled_at {dir}, seq_in_sec {dir}, id {dir} LIMIT ? OFFSET ?"
            ))
            .map_err(storage)?;
        let items = stmt
            .query_map(params_from_iter(args.iter()), record_from_row)
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)?;
        Ok(Page {
            items,
            total,
            page: q.page,
            page_size: size,
        })
    }

    fn timeline(&self, account: &AccountId, pool: Option<&PoolTypeId>) -> Result<Vec<GachaRecord>> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {RECORD_COLUMNS} FROM gacha_record WHERE account_id = ?1 AND (?2 IS NULL OR pool_type = ?2) \
                 ORDER BY pulled_at, seq_in_sec, id"
            ))
            .map_err(storage)?;
        let rows = stmt
            .query_map(
                params![account.as_str(), pool.map(PoolTypeId::as_str)],
                record_from_row,
            )
            .map_err(storage)?;
        rows.collect::<rusqlite::Result<_>>().map_err(storage)
    }

    fn latest_cursor(&self, account: &AccountId, pool: &PoolTypeId) -> Result<Option<Cursor>> {
        self.conn()
            .query_row(
                "SELECT ext_id, pulled_at, seq_in_sec FROM gacha_record WHERE account_id = ?1 AND pool_type = ?2 \
                 ORDER BY pulled_at DESC, seq_in_sec DESC, id DESC LIMIT 1",
                params![account.as_str(), pool.as_str()],
                |r| {
                    Ok(Cursor {
                        ext_id: r.get(0)?,
                        pulled_at: r.get(1)?,
                        seq_in_sec: r.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(storage)
    }

    fn backup(&self, dest: &Path) -> Result<()> {
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        self.conn()
            .backup(rusqlite::MAIN_DB, dest, None)
            .map_err(storage)
    }
}

#[cfg(test)]
mod tests;
