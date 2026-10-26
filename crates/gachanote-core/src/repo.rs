//! 存储接口（架构文档 4.3）。上层只依赖这些 trait，当前实现为本地 SQLite（`gachanote-store`），
//! 以后可以换成远程服务。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::ids::{AccountId, GameId, PoolTypeId};
use crate::model::{
    Account, AccountImport, Cursor, GachaRecord, ImportMeta, InsertSummary, NewAccount, Page,
    RecordQuery,
};
use crate::task::CancelToken;

/// 抽卡记录仓库。
pub trait RecordRepository: Send + Sync {
    /// 账号列表，可按游戏过滤，按创建时间排序。
    fn accounts(&self, game: Option<&GameId>) -> Result<Vec<Account>>;
    /// 按 ID 取账号。
    fn account(&self, id: &AccountId) -> Result<Option<Account>>;
    /// 新建账号；`(game, uid)` 已存在时补全为空的字段并返回已有账号。
    fn upsert_account(&self, account: &NewAccount) -> Result<Account>;
    /// 修改备注名，`None` 表示清除。
    fn rename_account(&self, id: &AccountId, nickname: Option<&str>) -> Result<()>;
    /// 删除账号及其全部记录。
    fn delete_account(&self, id: &AccountId) -> Result<()>;
    /// 在单个事务中写入若干账号的记录：自动建账号、按两条唯一索引去重、为每个账号写一条导入批次。
    /// 任一错误或 `cancel` 被置位都会整体回滚。
    fn insert_records(
        &self,
        meta: &ImportMeta,
        data: &[AccountImport],
        cancel: &CancelToken,
    ) -> Result<InsertSummary>;
    /// 分页查询记录。
    fn records(&self, query: &RecordQuery) -> Result<Page<GachaRecord>>;
    /// 某账号某卡池类型的全部记录，按时间顺序（统计用）。`pool = None` 表示全部卡池。
    fn timeline(&self, account: &AccountId, pool: Option<&PoolTypeId>) -> Result<Vec<GachaRecord>>;
    /// 某账号某卡池类型最新的一条记录（增量拉取用）。
    fn latest_cursor(&self, account: &AccountId, pool: &PoolTypeId) -> Result<Option<Cursor>>;
    /// 用 Online Backup 把整个数据库复制到 `dest`。
    fn backup(&self, dest: &Path) -> Result<()>;
}

/// 备份触发原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupReason {
    /// 导入或拉取前。
    Import,
    /// 数据库迁移前。
    Migration,
    /// 删除账号前。
    DeleteAccount,
    /// 用户手动。
    Manual,
}

/// 一份备份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupInfo {
    /// 文件名，如 `gachanote-20261025-120000.db`。
    pub name: String,
    /// 完整路径。
    pub path: PathBuf,
    /// 创建时间（UTC 毫秒）。
    pub created_at: i64,
    /// 文件大小（字节）。
    pub size: u64,
}

/// 自动备份：写入前调用，保留最近若干份。
pub trait BackupService: Send + Sync {
    /// 立即做一份备份。
    fn snapshot(&self, reason: BackupReason) -> Result<BackupInfo>;
}
