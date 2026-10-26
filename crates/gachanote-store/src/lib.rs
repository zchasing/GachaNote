//! GachaNote 存储层：`RecordRepository` 的 SQLite 实现、建表迁移与备份恢复。
//!
//! 单个 SQLite 文件，开启 WAL 与外键；连接放在互斥锁里，所有操作同步执行
//! （外壳在专用线程或任务线程上调用）。

mod backup;
mod sqlite;

pub use backup::{BackupManager, DEFAULT_KEEP};
pub use sqlite::{AccountActivity, MigrationBackup, SqliteStore, SCHEMA_VERSION};
