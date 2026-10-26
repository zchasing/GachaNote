//! 统一导入管线（架构文档 7.1）：获取 → 解码与规范化 → 校验 → 备份 → 单事务写入 → 报告。
//!
//! 文件导入、URL 拉取、token 拉取都产出 `Vec<ImportedAccount>`，从 [`ImportService::import_accounts`]
//! 开始走同一条路径。

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::exchange::{ImportedAccount, ImportedRecord, PoolRef, RecordFailure};
use crate::ids::{AccountId, FormatId, GameId, PoolTypeId};
use crate::model::{AccountImport, ImportMeta, ImportSource, NewAccount, NewRecord};
use crate::plugin::{FetchMode, FullFetch, GamePlugin, PluginRegistry, Since, SinceLookup};
use crate::repo::{BackupReason, BackupService, RecordRepository};
use crate::task::{
    CancelToken, Progress, ProgressEvent, Stage, TaskContext, TaskEvents, TaskHandle, TaskManager,
};

/// 识别格式时读取的文件开头字节数。
pub const SNIFF_LEN: usize = 4096;

/// 2000-01-01T00:00:00Z，早于它的时间视为不合理。
const MIN_TIME_MS: i64 = 946_684_800_000;
/// 允许比当前时间晚的毫秒数（时区换算误差等）。
const FUTURE_TOLERANCE_MS: i64 = 24 * 3600 * 1000;
/// 写入阶段每多少条检查一次取消、报告一次进度。
pub const WRITE_CHUNK: usize = 1000;

/// 当前 UTC 毫秒。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// 一条失败记录（导入报告用，FR-IMP-07）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureReport {
    /// 游戏。
    pub game: GameId,
    /// UID。
    pub uid: String,
    /// 位置。
    pub location: String,
    /// 原因。
    pub reason: String,
}

/// 单个账号的导入结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountReport {
    /// 账号 ID。
    pub account_id: AccountId,
    /// 游戏。
    pub game: GameId,
    /// UID。
    pub uid: String,
    /// 是否新建的账号（FR-ACC-02）。
    pub created: bool,
    /// 新增条数。
    pub added: u32,
    /// 重复跳过的条数。
    pub skipped: u32,
    /// 校验失败的条数。
    pub failed: u32,
}

/// 导入报告（`task://done` 的 `summary`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    /// 各账号结果。
    pub accounts: Vec<AccountReport>,
    /// 失败条目明细。
    pub failures: Vec<FailureReport>,
    /// 写入前做的备份文件名。
    pub backup: Option<String>,
}

impl ImportReport {
    /// 新增总数。
    pub fn added(&self) -> u32 {
        self.accounts.iter().map(|a| a.added).sum()
    }

    /// 重复总数。
    pub fn skipped(&self) -> u32 {
        self.accounts.iter().map(|a| a.skipped).sum()
    }

    /// 失败总数。
    pub fn failed(&self) -> u32 {
        u32::try_from(self.failures.len()).unwrap_or(u32::MAX)
    }

    /// 涉及的账号 ID（`data://changed` 的载荷）。
    pub fn account_ids(&self) -> Vec<AccountId> {
        self.accounts.iter().map(|a| a.account_id.clone()).collect()
    }
}

/// 文件预览中的一个账号（`sniff_file`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewAccount {
    /// 游戏。
    pub game: GameId,
    /// UID。
    pub uid: String,
    /// 可导入的条数。
    pub count: u32,
    /// 无法解码的条数。
    pub failed: u32,
    /// 最早记录时间（UTC 毫秒）。
    pub from: Option<i64>,
    /// 最晚记录时间（UTC 毫秒）。
    pub to: Option<i64>,
}

/// 文件预览。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePreview {
    /// 识别出的格式。
    pub format: FormatId,
    /// 文件中的账号。
    pub accounts: Vec<PreviewAccount>,
}

/// 规范化结果。
#[derive(Debug, Default)]
pub struct Normalized {
    /// 可写入的数据。
    pub imports: Vec<AccountImport>,
    /// 失败条目。
    pub failures: Vec<FailureReport>,
}

fn failure(acc: &ImportedAccount, f: RecordFailure) -> FailureReport {
    FailureReport {
        game: acc.game.clone(),
        uid: acc.uid.clone(),
        location: f.location,
        reason: f.reason,
    }
}

fn validate(rec: &ImportedRecord, max_rarity: u8, now: i64) -> std::result::Result<(), String> {
    if rec.item_name.trim().is_empty() {
        return Err("missing item name".into());
    }
    if rec.rarity == 0 || rec.rarity > max_rarity {
        return Err(format!(
            "rarity {} out of range 1..={max_rarity}",
            rec.rarity
        ));
    }
    if rec.pulled_at < MIN_TIME_MS || rec.pulled_at > now + FUTURE_TOLERANCE_MS {
        return Err(format!("unreasonable time {}", rec.pulled_at));
    }
    Ok(())
}

/// 比较官方记录 ID：数字 ID 按数值，其他按字符串。
fn cmp_ext_id(a: &Option<String>, b: &Option<String>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => (a.len(), a).cmp(&(b.len(), b)),
        _ => std::cmp::Ordering::Equal,
    }
}

/// 计算同秒序号：同一卡池类型、同一秒内按来源自带序号或官方 ID 排序后编号。
fn assign_seq(records: &mut [(ImportedRecord, PoolTypeId)]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..records.len()).collect();
    order.sort_by(|&i, &j| {
        let (a, pa) = &records[i];
        let (b, pb) = &records[j];
        pa.cmp(pb)
            .then((a.pulled_at / 1000).cmp(&(b.pulled_at / 1000)))
            .then(a.seq.cmp(&b.seq))
            .then(cmp_ext_id(&a.ext_id, &b.ext_id))
            .then(i.cmp(&j))
    });
    let mut seq = vec![0u32; records.len()];
    let mut prev: Option<(&PoolTypeId, i64)> = None;
    let mut n = 0u32;
    for &i in &order {
        let (r, p) = &records[i];
        let key = (p, r.pulled_at / 1000);
        n = if prev == Some(key) { n + 1 } else { 0 };
        prev = Some(key);
        seq[i] = r.seq.unwrap_or(n);
    }
    seq
}

fn normalize_account(
    plugin: &dyn GamePlugin,
    acc: ImportedAccount,
    now: i64,
    out: &mut Normalized,
) {
    if !plugin.validate_uid(&acc.uid) {
        out.failures.push(failure(
            &acc,
            RecordFailure::new("uid", format!("invalid uid for {}", acc.game)),
        ));
        return;
    }
    let max_rarity = plugin
        .rules()
        .iter()
        .map(|r| r.rarity_top)
        .max()
        .unwrap_or(6);
    for f in acc.failures.iter().cloned() {
        out.failures.push(failure(&acc, f));
    }
    let mut ok = Vec::with_capacity(acc.records.len());
    for rec in &acc.records {
        let pool = match &rec.pool {
            PoolRef::Official(code) => plugin.pool_type(code),
            PoolRef::Normalized(p) => plugin
                .rules()
                .iter()
                .any(|r| &r.pool_type == p)
                .then(|| p.clone()),
        };
        let Some(pool) = pool else {
            let code = match &rec.pool {
                PoolRef::Official(c) => c.as_str(),
                PoolRef::Normalized(p) => p.as_str(),
            };
            out.failures.push(failure(
                &acc,
                RecordFailure::new(rec.location.clone(), format!("unknown pool type {code}")),
            ));
            continue;
        };
        if let Err(reason) = validate(rec, max_rarity, now) {
            out.failures.push(failure(
                &acc,
                RecordFailure::new(rec.location.clone(), reason),
            ));
            continue;
        }
        ok.push((rec.clone(), pool));
    }
    let seq = assign_seq(&mut ok);
    let records = ok
        .into_iter()
        .zip(seq)
        .map(|((r, pool_type), seq_in_sec)| NewRecord {
            pool_type,
            banner_id: r.banner_id,
            ext_id: r.ext_id,
            item_id: r.item_id,
            item_name: r.item_name,
            item_type: r.item_type,
            rarity: r.rarity,
            pulled_at: r.pulled_at,
            seq_in_sec,
            is_pull: r.is_pull,
            raw_json: r.raw.to_string(),
        })
        .collect();
    out.imports.push(AccountImport {
        account: NewAccount {
            game: acc.game,
            uid: acc.uid,
            region: acc.region,
            nickname: acc.nickname,
            tz_offset_min: acc.tz_offset_min,
        },
        records,
    });
}

/// 规范化与校验：卡池类型映射、计算同秒序号、检查必填字段、星级与时间。
/// 不合格的记录进入失败列表，不中断整批。同一账号在输入中出现多次时合并。
pub fn normalize(
    registry: &PluginRegistry,
    accounts: Vec<ImportedAccount>,
    now: i64,
) -> Normalized {
    let mut merged: BTreeMap<(GameId, String), ImportedAccount> = BTreeMap::new();
    let mut order = Vec::new();
    for acc in accounts {
        let key = (acc.game.clone(), acc.uid.clone());
        match merged.get_mut(&key) {
            Some(existing) => {
                existing.records.extend(acc.records);
                existing.failures.extend(acc.failures);
            }
            None => {
                order.push(key.clone());
                merged.insert(key, acc);
            }
        }
    }
    let mut out = Normalized::default();
    for key in order {
        let acc = merged.remove(&key).unwrap_or_else(|| unreachable!());
        match registry.game(&acc.game) {
            Some(plugin) => normalize_account(plugin.as_ref(), acc, now, &mut out),
            None => out.failures.push(failure(
                &acc,
                RecordFailure::new("game", format!("unsupported game {}", acc.game)),
            )),
        }
    }
    out
}

/// 读文件开头并识别格式。
pub fn sniff_head(path: &Path) -> Result<Vec<u8>> {
    let mut head = Vec::with_capacity(SNIFF_LEN);
    File::open(path)?
        .take(SNIFF_LEN as u64)
        .read_to_end(&mut head)?;
    Ok(head)
}

/// 按数据库中已有记录计算增量起点：账号存在时，取插件每种卡池类型的最新一条记录。
#[derive(Clone)]
pub struct RepoSince {
    registry: Arc<PluginRegistry>,
    repo: Arc<dyn RecordRepository>,
}

impl RepoSince {
    /// 组装。
    pub fn new(registry: Arc<PluginRegistry>, repo: Arc<dyn RecordRepository>) -> Self {
        Self { registry, repo }
    }
}

impl SinceLookup for RepoSince {
    fn since(&self, game: &GameId, uid: &str) -> Result<Since> {
        let mut since = Since::new();
        let Some(account) = self
            .repo
            .accounts(Some(game))?
            .into_iter()
            .find(|a| &a.game == game && a.uid == uid)
        else {
            return Ok(since);
        };
        let Some(plugin) = self.registry.game(game) else {
            return Ok(since);
        };
        for rule in plugin.rules() {
            if since.contains_key(&rule.pool_type) {
                continue;
            }
            if let Some(cursor) = self.repo.latest_cursor(&account.id, &rule.pool_type)? {
                since.insert(rule.pool_type.clone(), cursor);
            }
        }
        Ok(since)
    }
}

/// 导入服务：持有插件注册表、仓库与备份，供外壳的命令调用。
#[derive(Clone)]
pub struct ImportService {
    registry: Arc<PluginRegistry>,
    repo: Arc<dyn RecordRepository>,
    backup: Option<Arc<dyn BackupService>>,
}

impl ImportService {
    /// 组装。`backup` 为空时写入前不备份（测试用）。
    pub fn new(
        registry: Arc<PluginRegistry>,
        repo: Arc<dyn RecordRepository>,
        backup: Option<Arc<dyn BackupService>>,
    ) -> Self {
        Self {
            registry,
            repo,
            backup,
        }
    }

    /// 识别并解码文件。
    pub fn decode_file(&self, path: &Path) -> Result<(FormatId, Vec<ImportedAccount>)> {
        let head = sniff_head(path)?;
        let converter = match self.registry.sniff(&head) {
            Some(c) => c,
            // 有些导出工具把 info 放在文件末尾，开头识别不出时再看整个文件
            None if head.len() >= SNIFF_LEN => {
                let all = std::fs::read(path)?;
                self.registry.sniff(&all).ok_or(AppError::UnknownFormat)?
            }
            None => return Err(AppError::UnknownFormat),
        };
        let mut reader = BufReader::new(File::open(path)?);
        let accounts = converter.decode(&mut reader)?;
        Ok((converter.format(), accounts))
    }

    /// 文件预览：格式、账号、条数与时间范围（不写入）。
    pub fn preview_file(&self, path: &Path) -> Result<FilePreview> {
        let (format, accounts) = self.decode_file(path)?;
        let accounts = accounts
            .iter()
            .map(|a| PreviewAccount {
                game: a.game.clone(),
                uid: a.uid.clone(),
                count: u32::try_from(a.records.len()).unwrap_or(u32::MAX),
                failed: u32::try_from(a.failures.len()).unwrap_or(u32::MAX),
                from: a.records.iter().map(|r| r.pulled_at).min(),
                to: a.records.iter().map(|r| r.pulled_at).max(),
            })
            .collect();
        Ok(FilePreview { format, accounts })
    }

    /// 管线后半段：规范化 → 校验 → 备份 → 单事务写入。
    pub fn import_accounts(
        &self,
        accounts: Vec<ImportedAccount>,
        meta: &ImportMeta,
        progress: &dyn Progress,
        cancel: &CancelToken,
    ) -> Result<ImportReport> {
        cancel.check()?;
        let total: usize = accounts.iter().map(|a| a.records.len()).sum();
        progress.report(ProgressEvent {
            stage: Stage::Parse,
            current: total as u64,
            total: Some(total as u64),
            message: String::new(),
        });
        let normalized = normalize(&self.registry, accounts, now_ms());
        cancel.check()?;

        let mut report = ImportReport {
            failures: normalized.failures,
            ..ImportReport::default()
        };
        if normalized.imports.is_empty() {
            return Ok(report);
        }

        if let Some(backup) = &self.backup {
            progress.report(ProgressEvent {
                stage: Stage::Backup,
                current: 0,
                total: None,
                message: String::new(),
            });
            report.backup = Some(backup.snapshot(BackupReason::Import)?.name);
        }
        cancel.check()?;

        let write_total: usize = normalized.imports.iter().map(|a| a.records.len()).sum();
        progress.report(ProgressEvent {
            stage: Stage::Write,
            current: 0,
            total: Some(write_total as u64),
            message: String::new(),
        });
        let summary = self
            .repo
            .insert_records(meta, &normalized.imports, cancel)?;
        progress.report(ProgressEvent {
            stage: Stage::Write,
            current: write_total as u64,
            total: Some(write_total as u64),
            message: String::new(),
        });

        let mut failed: HashMap<(GameId, String), u32> = HashMap::new();
        for f in &report.failures {
            *failed.entry((f.game.clone(), f.uid.clone())).or_default() += 1;
        }
        report.accounts = summary
            .accounts
            .into_iter()
            .map(|a| AccountReport {
                failed: failed
                    .get(&(a.account.game.clone(), a.account.uid.clone()))
                    .copied()
                    .unwrap_or(0),
                account_id: a.account.id,
                game: a.account.game,
                uid: a.account.uid,
                created: a.created,
                added: a.added,
                skipped: a.skipped,
            })
            .collect();
        Ok(report)
    }

    /// 同步导入一个文件。
    pub fn import_file(
        &self,
        path: &Path,
        progress: &dyn Progress,
        cancel: &CancelToken,
    ) -> Result<ImportReport> {
        let started_at = now_ms();
        let (format, accounts) = self.decode_file(path)?;
        let meta = ImportMeta {
            source: ImportSource::File,
            format: Some(format),
            file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            started_at,
        };
        self.import_accounts(accounts, &meta, progress, cancel)
    }

    /// 拉取用的增量起点查询。
    pub fn since_lookup(&self, mode: FetchMode) -> Box<dyn SinceLookup> {
        match mode {
            FetchMode::Incremental => Box::new(RepoSince::new(
                Arc::clone(&self.registry),
                Arc::clone(&self.repo),
            )),
            FetchMode::Full => Box::new(FullFetch),
        }
    }

    /// 在后台任务中拉取并导入（`fetch_by_url` 等命令）。
    ///
    /// `fetch` 在任务线程上运行，负责调用拉取器（外壳在其中 `block_on` 异步拉取），
    /// 拿到记录后与文件导入走同一条管线：占用账号 → 规范化 → 备份 → 单事务写入。
    /// 取消时已拉取的部分不写入（FR-FET-07）。
    pub fn spawn_fetch<F>(
        &self,
        tasks: &TaskManager,
        events: Arc<dyn TaskEvents>,
        mode: FetchMode,
        source: ImportSource,
        fetch: F,
    ) -> TaskHandle
    where
        F: FnOnce(&TaskContext, &dyn SinceLookup) -> Result<Vec<ImportedAccount>> + Send + 'static,
    {
        let service = self.clone();
        tasks.spawn(events, move |ctx| {
            let started_at = now_ms();
            let since = service.since_lookup(mode);
            let accounts = fetch(ctx, since.as_ref())?;
            ctx.cancel_token().check()?;
            let _claim = ctx.claim(accounts.iter().map(|a| format!("{}:{}", a.game, a.uid)))?;
            let meta = ImportMeta {
                source,
                format: None,
                file_name: None,
                started_at,
            };
            service.import_accounts(accounts, &meta, ctx, ctx.cancel_token())
        })
    }

    /// 在后台任务中导入文件（`import_file` 命令）。解码后占用涉及的账号，
    /// 同一账号已有写入任务时以 [`AppError::Busy`] 失败。
    pub fn spawn_import_file(
        &self,
        tasks: &TaskManager,
        events: Arc<dyn TaskEvents>,
        path: PathBuf,
    ) -> TaskHandle {
        let service = self.clone();
        tasks.spawn(events, move |ctx| {
            let started_at = now_ms();
            let (format, accounts) = service.decode_file(&path)?;
            let _claim = ctx.claim(accounts.iter().map(|a| format!("{}:{}", a.game, a.uid)))?;
            let meta = ImportMeta {
                source: ImportSource::File,
                format: Some(format),
                file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
                started_at,
            };
            service.import_accounts(accounts, &meta, ctx, ctx.cancel_token())
        })
    }
}

#[cfg(test)]
mod tests;
