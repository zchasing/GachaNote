//! 应用状态：组装插件、存储、备份、元数据与任务管理（架构文档 2.1，`src-tauri` 是唯一的组装点）。
//!
//! 命令的业务逻辑都写成 [`AppState`] 的方法，不依赖 Tauri 运行时，便于单元测试；
//! `commands.rs` 只负责参数转换与事件转发。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use gachanote_core::exchange::ExportAccount;
use gachanote_core::import::ImportService;
use gachanote_core::model::{ImportSource, RecordQuery, SortOrder};
use gachanote_core::plugin::{FetchMode, FetchSource, PluginRegistry};
use gachanote_core::repo::{BackupReason, BackupService, RecordRepository};
use gachanote_core::stats::{account_stats, record_marks, RecordMark};
use gachanote_core::task::{TaskEvents, TaskId, TaskManager};
use gachanote_core::{AccountId, AppError, FormatId, GameId, RecordId, Redacted, Result};
use gachanote_games::{Pacing, StandardPaths};
use gachanote_meta::{MetaSet, MetaStore, UpdateOutcome};
use gachanote_store::{BackupManager, SqliteStore, DEFAULT_KEEP};

use crate::dto::{
    AccountDto, AccountKey, AccountStatsDto, AppInfo, BackupDto, DirKind, ExportFormat,
    ExportResult, FetchModeDto, FilePreviewDto, GameDto, MetaInfo, MetaUpdateResult, OverviewEntry,
    RecordDto, RecordPage, RecordQueryDto, Settings, UrlSource,
};

/// 数据库文件名。
pub const DB_FILE: &str = "gachanote.db";
/// 记录自定义数据目录的文件（放在默认目录，因为数据库本身可能被搬走）。
const LOCATION_FILE: &str = "location.json";
/// 设置在 `setting` 表中的键。
const SETTINGS_KEY: &str = "app.settings";

/// 各目录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// 默认目录（`%APPDATA%/GachaNote`）：日志、元数据缓存、`location.json`。
    pub base: PathBuf,
    /// 数据目录：数据库与备份，默认同 `base`，可在设置里更改（F-23）。
    pub data: PathBuf,
}

impl Paths {
    /// 读取 `base/location.json` 决定数据目录；文件缺失或损坏时用默认目录。
    pub fn resolve(base: PathBuf) -> Self {
        let data = fs::read(base.join(LOCATION_FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("dataDir")?.as_str().map(PathBuf::from))
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| base.clone());
        Self { base, data }
    }

    /// 数据库文件。
    pub fn db(&self) -> PathBuf {
        self.data.join(DB_FILE)
    }

    /// 备份目录。
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }

    /// 日志目录。
    pub fn logs(&self) -> PathBuf {
        self.base.join("logs")
    }

    /// 元数据缓存目录。
    pub fn metadata(&self) -> PathBuf {
        self.base.join("metadata")
    }
}

/// 后端发给前端的事件，由外壳转成 tauri-specta 事件。
#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    /// 任务进度。
    Progress(crate::dto::TaskProgress),
    /// 任务完成。
    Done(crate::dto::TaskDone),
    /// 任务失败。
    Failed(crate::dto::TaskFailed),
    /// 数据有写入。
    DataChanged(Vec<String>),
    /// 卡池数据包已更新。
    MetaUpdated(String),
}

/// 事件出口。
pub trait EventSink: Send + Sync {
    /// 发出一个事件。
    fn emit(&self, event: AppEvent);
}

/// 一个账号的统计与记录标注，按需计算、写入后失效。
struct Computed {
    stats: AccountStatsDto,
    marks: HashMap<RecordId, RecordMark>,
}

type Cache = Arc<Mutex<HashMap<AccountId, Arc<Computed>>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 把任务事件转给 [`EventSink`]，并在写入完成后让缓存失效。
struct TaskBridge {
    sink: Arc<dyn EventSink>,
    cache: Cache,
}

impl TaskEvents for TaskBridge {
    fn progress(&self, id: TaskId, event: gachanote_core::task::ProgressEvent) {
        self.sink
            .emit(AppEvent::Progress(crate::dto::TaskProgress::new(id, event)));
    }

    fn done(&self, id: TaskId, report: &gachanote_core::import::ImportReport) {
        let ids = report.account_ids();
        {
            let mut cache = lock(&self.cache);
            for a in &ids {
                cache.remove(a);
            }
        }
        self.sink.emit(AppEvent::Done(crate::dto::TaskDone {
            task_id: id,
            report: report.into(),
        }));
        if !ids.is_empty() {
            self.sink.emit(AppEvent::DataChanged(
                ids.iter().map(ToString::to_string).collect(),
            ));
        }
    }

    fn failed(&self, id: TaskId, error: &AppError) {
        self.sink.emit(AppEvent::Failed(crate::dto::TaskFailed {
            task_id: id,
            error: error.clone().into(),
        }));
    }
}

/// 应用状态。
pub struct AppState {
    paths: Paths,
    registry: Arc<PluginRegistry>,
    store: Arc<SqliteStore>,
    backups: Arc<BackupManager>,
    import: ImportService,
    tasks: TaskManager,
    meta: RwLock<Arc<MetaSet>>,
    meta_store: MetaStore,
    client: reqwest::Client,
    sink: Arc<dyn EventSink>,
    cache: Cache,
    discovered: Mutex<HashMap<String, FetchSource>>,
    next_source: AtomicU64,
}

/// 按代理设置创建 HTTP 客户端：15 秒超时；没有自定义代理时跟随系统代理。
pub fn http_client(proxy: Option<&str>) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(concat!("GachaNote/", env!("CARGO_PKG_VERSION")));
    if let Some(p) = proxy.filter(|p| !p.trim().is_empty()) {
        match reqwest::Proxy::all(p.trim()) {
            Ok(proxy) => b = b.proxy(proxy),
            Err(e) => tracing::warn!(error = %e, "代理地址无效，改用系统代理"),
        }
    }
    b.build().unwrap_or_default()
}

impl AppState {
    /// 打开数据库（必要时迁移前备份）、加载元数据包、注册六款游戏。
    pub fn open(paths: Paths, sink: Arc<dyn EventSink>) -> Result<Self> {
        let backup_dir = paths.backups();
        let store = SqliteStore::open_with_backup(
            &paths.db(),
            Some(&|s: &SqliteStore| {
                BackupManager::snapshot_into(&backup_dir, DEFAULT_KEEP, s, BackupReason::Migration)
                    .map(|_| ())
            }),
        )?;
        Self::with_store(paths, Arc::new(store), Pacing::default(), sink)
    }

    /// 用已打开的存储组装（测试用内存数据库）。
    pub fn with_store(
        paths: Paths,
        store: Arc<SqliteStore>,
        pacing: Pacing,
        sink: Arc<dyn EventSink>,
    ) -> Result<Self> {
        let settings = read_settings(&store)?;
        let client = http_client(settings.proxy.as_deref());
        let mut registry = PluginRegistry::new();
        gachanote_games::register_all(&mut registry, &client, &pacing);
        let registry = Arc::new(registry);
        let backups = Arc::new(BackupManager::new(
            paths.backups(),
            DEFAULT_KEEP,
            Arc::clone(&store),
        ));
        let import = ImportService::new(
            Arc::clone(&registry),
            Arc::clone(&store) as Arc<dyn RecordRepository>,
            Some(Arc::clone(&backups) as Arc<dyn BackupService>),
        );
        let meta_store = MetaStore::new(paths.metadata());
        let meta = meta_store.load();
        tracing::info!(version = meta.version(), origin = ?meta.origin, "已加载卡池数据包");
        Ok(Self {
            paths,
            registry,
            store,
            backups,
            import,
            tasks: TaskManager::new(),
            meta: RwLock::new(Arc::new(meta)),
            meta_store,
            client,
            sink,
            cache: Cache::default(),
            discovered: Mutex::new(HashMap::new()),
            next_source: AtomicU64::new(0),
        })
    }

    fn bridge(&self) -> Arc<dyn TaskEvents> {
        Arc::new(TaskBridge {
            sink: Arc::clone(&self.sink),
            cache: Arc::clone(&self.cache),
        })
    }

    fn invalidate(&self, ids: &[AccountId]) {
        let mut cache = lock(&self.cache);
        if ids.is_empty() {
            cache.clear();
        } else {
            for id in ids {
                cache.remove(id);
            }
        }
    }

    fn changed(&self, ids: &[AccountId]) {
        self.invalidate(ids);
        self.sink.emit(AppEvent::DataChanged(
            ids.iter().map(ToString::to_string).collect(),
        ));
    }

    fn meta(&self) -> Arc<MetaSet> {
        Arc::clone(&self.meta.read().unwrap_or_else(PoisonError::into_inner))
    }

    fn account(&self, id: &str) -> Result<gachanote_core::model::Account> {
        self.store
            .account(&AccountId::from(id))?
            .ok_or_else(|| AppError::NotFound(format!("account {id}")))
    }

    fn plugin(&self, game: &GameId) -> Result<&Arc<dyn gachanote_core::plugin::GamePlugin>> {
        self.registry
            .game(game)
            .ok_or_else(|| AppError::Invalid(format!("unsupported game {game}")))
    }

    // ---------- 游戏与账号 ----------

    /// 全部游戏，按边栏顺序。
    pub fn list_games(&self) -> Vec<GameDto> {
        self.registry
            .games()
            .iter()
            .map(|p| GameDto::from_plugin(p.as_ref()))
            .collect()
    }

    /// 全部账号，按创建时间。
    pub fn list_accounts(&self) -> Result<Vec<AccountDto>> {
        let activity: HashMap<AccountId, (u64, Option<i64>)> = self
            .store
            .account_activity()?
            .into_iter()
            .map(|a| (a.account_id, (a.records, a.last_import_at)))
            .collect();
        Ok(self
            .store
            .accounts(None)?
            .into_iter()
            .map(|a| {
                let (records, last) = activity.get(&a.id).copied().unwrap_or((0, None));
                AccountDto::new(a, records, last)
            })
            .collect())
    }

    /// 修改备注名，空字符串表示清除。
    pub fn rename_account(&self, id: &str, nickname: Option<&str>) -> Result<()> {
        let nickname = nickname.map(str::trim).filter(|n| !n.is_empty());
        self.store.rename_account(&id.into(), nickname)?;
        self.sink.emit(AppEvent::DataChanged(vec![id.to_owned()]));
        Ok(())
    }

    /// 删除账号及其全部记录，删除前先备份。
    pub fn delete_account(&self, id: &str) -> Result<()> {
        let id = AccountId::from(id);
        self.store
            .delete_account_with_backup(&id, self.backups.as_ref())?;
        self.changed(&[id]);
        Ok(())
    }

    // ---------- 统计与记录 ----------

    fn computed(&self, account_id: &str) -> Result<Arc<Computed>> {
        let id = AccountId::from(account_id);
        if let Some(c) = lock(&self.cache).get(&id) {
            return Ok(Arc::clone(c));
        }
        let account = self.account(account_id)?;
        let plugin = self.plugin(&account.game)?;
        let meta = self.meta().game(&account.game);
        let records = self.store.timeline(&id, None)?;
        let stats = account_stats(plugin.as_ref(), &meta, &records);
        let marks = record_marks(plugin.as_ref(), &meta, &records);
        let computed = Arc::new(Computed {
            stats: AccountStatsDto::new(account_id, plugin.as_ref(), &stats, &records),
            marks: records
                .iter()
                .zip(marks)
                .filter(|(_, m)| m.pulls.is_some() || m.up.is_some())
                .map(|(r, m)| (r.id.clone(), m))
                .collect(),
        });
        lock(&self.cache).insert(id, Arc::clone(&computed));
        Ok(computed)
    }

    /// 一个账号的统计（F-09–F-12）。
    pub fn get_stats(&self, account_id: &str) -> Result<AccountStatsDto> {
        Ok(self.computed(account_id)?.stats.clone())
    }

    /// 跨游戏总览（F-18）。
    pub fn get_overview(&self) -> Result<Vec<OverviewEntry>> {
        self.list_accounts()?
            .into_iter()
            .filter(|a| self.registry.game(&GameId::from(a.game.as_str())).is_some())
            .map(|account| {
                let stats = self.get_stats(&account.id)?;
                Ok(OverviewEntry { account, stats })
            })
            .collect()
    }

    /// 记录列表（F-13）：分页在数据库里做，当次抽数与 UP 标注来自缓存。
    pub fn query_records(&self, q: RecordQueryDto) -> Result<RecordPage> {
        let computed = self.computed(&q.account_id)?;
        let page = self.store.records(&RecordQuery {
            account_id: q.account_id.into(),
            pool_types: q.pool_types.into_iter().map(Into::into).collect(),
            rarities: q.rarities,
            from: q.from,
            to: q.to,
            name: q.name.filter(|n| !n.trim().is_empty()),
            order: SortOrder::NewestFirst,
            page: q.page,
            page_size: q.page_size,
        })?;
        Ok(RecordPage {
            items: page
                .items
                .into_iter()
                .map(|r| {
                    let mark = computed.marks.get(&r.id).copied().unwrap_or_default();
                    RecordDto::new(r, mark)
                })
                .collect(),
            total: page.total,
            page: page.page,
            page_size: page.page_size,
        })
    }

    // ---------- 导入与拉取 ----------

    /// 识别文件格式并预览账号与条数（不写入）。
    pub fn preview_file(&self, path: &Path) -> Result<FilePreviewDto> {
        let preview = self.import.preview_file(path)?;
        let existing: Vec<(String, String)> = self
            .store
            .accounts(None)?
            .into_iter()
            .map(|a| (a.game.to_string(), a.uid))
            .collect();
        Ok(FilePreviewDto::new(
            path,
            preview,
            |g, u| existing.iter().any(|(eg, eu)| eg == g && eu == u),
            |g| self.registry.game(&GameId::from(g)).is_some(),
        ))
    }

    /// 后台导入文件，只写入 `only` 中的账号（为空表示全部）。
    pub fn import_file(&self, path: PathBuf, only: Option<Vec<AccountKey>>) -> TaskId {
        let only = only.map(|v| {
            v.into_iter()
                .map(|k| (GameId::from(k.game), k.uid))
                .collect()
        });
        self.import
            .spawn_import_selected(&self.tasks, self.bridge(), path, only)
            .id
    }

    /// 在游戏缓存或日志里找抽卡链接。链接留在后端，返回编号与脱敏地址。
    pub async fn discover_url(&self, game: &str) -> Result<crate::dto::DiscoveredUrl> {
        let game = GameId::from(game);
        let plugin = Arc::clone(self.plugin(&game)?);
        let fetcher = plugin
            .fetcher()
            .ok_or_else(|| AppError::Invalid(format!("{game} cannot fetch by url")))?;
        let sources = fetcher.discover(&StandardPaths::from_env()).await?;
        let src = sources
            .into_iter()
            .next()
            .ok_or_else(|| AppError::UrlNotFound { game: game.clone() })?;
        let display = src
            .url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let id = format!(
            "src-{}",
            self.next_source.fetch_add(1, Ordering::SeqCst) + 1
        );
        lock(&self.discovered).insert(id.clone(), src);
        Ok(crate::dto::DiscoveredUrl {
            source_id: id,
            game: game.to_string(),
            display,
        })
    }

    fn fetch_source(&self, game: &GameId, source: UrlSource) -> Result<FetchSource> {
        match source {
            UrlSource::Discovered { id } => lock(&self.discovered)
                .get(&id)
                .filter(|s| &s.game == game)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("source {id}"))),
            UrlSource::Pasted { url } => {
                let url = url.trim();
                match gachanote_games::detect_url(url) {
                    Some(g) if &g == game => Ok(FetchSource {
                        game: game.clone(),
                        url: Some(Redacted::url(url.to_owned())),
                        account: None,
                    }),
                    Some(other) => Err(AppError::Invalid(format!(
                        "url belongs to {other}, not {game}"
                    ))),
                    None => Err(AppError::Invalid("not a gacha record url".into())),
                }
            }
        }
    }

    /// 后台按链接拉取并写入（F-03）。
    pub fn fetch_by_url(
        &self,
        game: &str,
        source: UrlSource,
        mode: FetchModeDto,
    ) -> Result<TaskId> {
        let game = GameId::from(game);
        let plugin = Arc::clone(self.plugin(&game)?);
        if plugin.fetcher().is_none() {
            return Err(AppError::Invalid(format!("{game} cannot fetch by url")));
        }
        let src = self.fetch_source(&game, source)?;
        let mode = match mode {
            FetchModeDto::Incremental => FetchMode::Incremental,
            FetchModeDto::Full => FetchMode::Full,
        };
        let handle = self.import.spawn_fetch(
            &self.tasks,
            self.bridge(),
            mode,
            ImportSource::Url,
            move |ctx, since| {
                let fetcher = plugin
                    .fetcher()
                    .ok_or_else(|| AppError::Internal("fetcher missing".into()))?;
                tauri::async_runtime::block_on(fetcher.fetch(&src, since, ctx, ctx.cancel_token()))
            },
        );
        Ok(handle.id)
    }

    /// 取消导入或拉取；任务已结束时返回 `false`。
    pub fn cancel_task(&self, id: TaskId) -> bool {
        self.tasks.cancel(id)
    }

    // ---------- 导出 ----------

    /// 导出账号到文件（F-07）。UIGF 只能包含米哈游三款。
    pub fn export(
        &self,
        account_ids: &[String],
        format: ExportFormat,
        path: &Path,
    ) -> Result<ExportResult> {
        if account_ids.is_empty() {
            return Err(AppError::Invalid("no account selected".into()));
        }
        let format_id = match format {
            ExportFormat::Uigf => FormatId::UigfV4,
            ExportFormat::Gnj => FormatId::Gnj,
            ExportFormat::Csv => FormatId::GnCsv,
        };
        let converter = self
            .registry
            .converter(format_id)
            .ok_or_else(|| AppError::Internal(format!("converter {format_id} missing")))?;
        let mut data = Vec::new();
        for id in account_ids {
            let account = self.account(id)?;
            let records = self.store.timeline(&account.id, None)?;
            data.push(ExportAccount { account, records });
        }
        // 先写临时文件，成功后再替换，失败时不留半个文件
        let tmp = path.with_extension("gachanote-tmp");
        let result = (|| -> Result<()> {
            let mut out = BufWriter::new(fs::File::create(&tmp)?);
            converter.encode(&data, &mut out)?;
            out.flush()?;
            drop(out);
            fs::rename(&tmp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
        Ok(ExportResult {
            path: path.display().to_string(),
            accounts: u32::try_from(data.len()).unwrap_or(u32::MAX),
            records: data.iter().map(|a| a.records.len() as u64).sum(),
        })
    }

    // ---------- 设置 ----------

    /// 读取设置。
    pub fn get_settings(&self) -> Result<Settings> {
        read_settings(&self.store)
    }

    /// 保存设置。
    pub fn set_settings(&self, settings: &Settings) -> Result<()> {
        let value =
            serde_json::to_value(settings).map_err(|e| AppError::Internal(e.to_string()))?;
        self.store.set_setting(SETTINGS_KEY, &value)
    }

    /// 应用与目录信息。
    pub fn app_info(&self) -> AppInfo {
        AppInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            data_dir: self.paths.data.display().to_string(),
            default_data_dir: self.paths.base.display().to_string(),
            log_dir: self.paths.logs().display().to_string(),
            db_size: fs::metadata(self.paths.db()).map(|m| m.len()).unwrap_or(0),
            can_discover: cfg!(windows),
        }
    }

    /// 目录路径。
    pub fn dir(&self, kind: DirKind) -> PathBuf {
        match kind {
            DirKind::Data => self.paths.data.clone(),
            DirKind::Backups => self.paths.backups(),
            DirKind::Logs => self.paths.logs(),
        }
    }

    /// 把数据库与备份复制到新目录，并记下新位置；重启后生效（F-23）。
    /// 原目录的文件保留不动，由用户确认后自行删除。
    pub fn set_data_dir(&self, dir: &Path) -> Result<String> {
        if !dir.is_absolute() {
            return Err(AppError::Invalid("data dir must be absolute".into()));
        }
        let target = if dir == self.paths.base {
            None
        } else {
            Some(dir.to_path_buf())
        };
        if dir != self.paths.data {
            let db = dir.join(DB_FILE);
            if db.exists() {
                return Err(AppError::Invalid(format!(
                    "{} already exists",
                    db.display()
                )));
            }
            fs::create_dir_all(dir)?;
            self.store.backup(&db)?;
            let backups = self.backups.list()?;
            if !backups.is_empty() {
                let dest = dir.join("backups");
                fs::create_dir_all(&dest)?;
                for b in backups {
                    fs::copy(&b.path, dest.join(&b.name))?;
                }
            }
        }
        let location = self.paths.base.join(LOCATION_FILE);
        match &target {
            Some(t) => {
                fs::create_dir_all(&self.paths.base)?;
                fs::write(
                    &location,
                    serde_json::json!({ "dataDir": t.display().to_string() }).to_string(),
                )?;
            }
            None => {
                let _ = fs::remove_file(&location);
            }
        }
        tracing::info!(dir = %dir.display(), "数据目录已更改，重启后生效");
        Ok(dir.display().to_string())
    }

    // ---------- 备份 ----------

    /// 全部备份，最新的在前。
    pub fn list_backups(&self) -> Result<Vec<BackupDto>> {
        Ok(self
            .backups
            .list()?
            .into_iter()
            .map(|b| BackupDto {
                name: b.name,
                created_at: b.created_at,
                size: b.size,
            })
            .collect())
    }

    /// 手动备份。
    pub fn create_backup(&self) -> Result<BackupDto> {
        let b = self.backups.snapshot(BackupReason::Manual)?;
        Ok(BackupDto {
            name: b.name,
            created_at: b.created_at,
            size: b.size,
        })
    }

    /// 从备份恢复。恢复前先把当前数据备份一份，误操作还能找回。
    pub fn restore_backup(&self, name: &str) -> Result<()> {
        if !self.backups.list()?.iter().any(|b| b.name == name) {
            return Err(AppError::NotFound(format!("backup {name}")));
        }
        self.backups.snapshot(BackupReason::Manual)?;
        self.backups.restore(name)?;
        self.changed(&[]);
        Ok(())
    }

    // ---------- 卡池数据包 ----------

    /// 当前数据包。
    pub fn meta_info(&self) -> MetaInfo {
        let meta = self.meta();
        MetaInfo {
            version: meta.version().into(),
            origin: match meta.origin {
                gachanote_meta::PackOrigin::Bundled => "bundled",
                gachanote_meta::PackOrigin::Cache => "cache",
            }
            .into(),
        }
    }

    /// 检查并下载新的数据包；更新后重算全部统计并发 `meta://updated`（F-08）。
    pub async fn update_metadata(&self) -> Result<MetaUpdateResult> {
        let sources = gachanote_meta::default_sources(&self.client);
        let outcome = self.meta_store.update(&sources).await?;
        Ok(self.apply_meta_outcome(outcome))
    }

    fn apply_meta_outcome(&self, outcome: UpdateOutcome) -> MetaUpdateResult {
        match outcome {
            UpdateOutcome::Updated { version } => {
                *self.meta.write().unwrap_or_else(PoisonError::into_inner) =
                    Arc::new(self.meta_store.load());
                self.invalidate(&[]);
                self.sink.emit(AppEvent::MetaUpdated(version.clone()));
                MetaUpdateResult {
                    outcome: "updated".into(),
                    version,
                    min_app_version: None,
                }
            }
            UpdateOutcome::UpToDate { version } => MetaUpdateResult {
                outcome: "upToDate".into(),
                version,
                min_app_version: None,
            },
            UpdateOutcome::AppTooOld {
                version,
                min_app_version,
            } => MetaUpdateResult {
                outcome: "appTooOld".into(),
                version,
                min_app_version: Some(min_app_version),
            },
        }
    }
}

fn read_settings(store: &SqliteStore) -> Result<Settings> {
    let Some(value) = store.setting(SETTINGS_KEY)? else {
        return Ok(Settings::default());
    };
    // 字段缺失或类型不对时用默认值，不因为旧版本的设置打不开应用
    let mut s = Settings::default();
    let get = |k: &str| value.get(k).cloned();
    if let Some(v) = get("language").and_then(|v| v.as_str().map(String::from)) {
        s.language = v;
    }
    if let Some(v) = get("theme").and_then(|v| v.as_str().map(String::from)) {
        s.theme = v;
    }
    s.proxy = get("proxy").and_then(|v| v.as_str().map(String::from));
    if let Some(v) = get("sidebarGames").and_then(|v| serde_json::from_value(v).ok()) {
        s.sidebar_games = v;
    }
    if let Some(v) =
        get("lastAccounts").and_then(|v| serde_json::from_value::<BTreeMap<String, String>>(v).ok())
    {
        s.last_accounts = v;
    }
    if let Some(v) = get("metaAutoUpdate").and_then(|v| v.as_bool()) {
        s.meta_auto_update = v;
    }
    Ok(s)
}

#[cfg(test)]
mod tests;
