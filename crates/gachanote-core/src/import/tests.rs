use std::io::Write as _;
use std::sync::Mutex;

use super::*;
use crate::exchange::ExportAccount;
use crate::model::{Account, AccountInsert, Cursor, GachaRecord, InsertSummary, Page, RecordQuery};
use crate::plugin::testing::TestGame;
use crate::plugin::Converter;
use crate::repo::BackupInfo;
use crate::task::NoProgress;
use serde_json::json;

const T0: i64 = 1_700_000_000_000;

/// 内存仓库：按与 SQLite 实现相同的两种去重键去重。
#[derive(Default)]
struct MemRepo {
    accounts: Mutex<Vec<Account>>,
    records: Mutex<Vec<GachaRecord>>,
    fail_insert: bool,
}

impl RecordRepository for MemRepo {
    fn accounts(&self, _game: Option<&GameId>) -> Result<Vec<Account>> {
        Ok(self.accounts.lock().unwrap().clone())
    }
    fn account(&self, _id: &AccountId) -> Result<Option<Account>> {
        unimplemented!()
    }
    fn upsert_account(&self, _a: &NewAccount) -> Result<Account> {
        unimplemented!()
    }
    fn rename_account(&self, _id: &AccountId, _n: Option<&str>) -> Result<()> {
        unimplemented!()
    }
    fn delete_account(&self, _id: &AccountId) -> Result<()> {
        unimplemented!()
    }
    fn insert_records(
        &self,
        _meta: &ImportMeta,
        data: &[AccountImport],
        cancel: &CancelToken,
    ) -> Result<InsertSummary> {
        if self.fail_insert {
            return Err(AppError::Storage("disk full".into()));
        }
        let mut accounts = self.accounts.lock().unwrap().clone();
        let mut records = self.records.lock().unwrap().clone();
        let mut summary = InsertSummary::default();
        for (n, imp) in data.iter().enumerate() {
            let (account, created) = match accounts
                .iter()
                .find(|a| a.game == imp.account.game && a.uid == imp.account.uid)
            {
                Some(a) => (a.clone(), false),
                None => {
                    let a = Account {
                        id: format!("acc-{}", accounts.len()).into(),
                        game: imp.account.game.clone(),
                        uid: imp.account.uid.clone(),
                        region: imp.account.region.clone(),
                        nickname: imp.account.nickname.clone(),
                        tz_offset_min: imp.account.tz_offset_min,
                        created_at: 0,
                    };
                    accounts.push(a.clone());
                    (a, true)
                }
            };
            let (mut added, mut skipped) = (0, 0);
            for r in &imp.records {
                let dup = records.iter().any(|x| {
                    x.account_id == account.id
                        && match (&r.ext_id, &x.data.ext_id) {
                            (Some(a), Some(b)) => a == b,
                            (None, None) => {
                                (
                                    &x.data.pool_type,
                                    x.data.pulled_at,
                                    &x.data.item_name,
                                    x.data.seq_in_sec,
                                ) == (&r.pool_type, r.pulled_at, &r.item_name, r.seq_in_sec)
                            }
                            _ => false,
                        }
                });
                if dup {
                    skipped += 1;
                } else {
                    added += 1;
                    records.push(GachaRecord {
                        id: format!("r{}", records.len()).into(),
                        account_id: account.id.clone(),
                        data: r.clone(),
                        batch_id: None,
                    });
                }
            }
            summary.accounts.push(AccountInsert {
                account,
                created,
                batch_id: format!("b{n}").into(),
                added,
                skipped,
            });
            cancel.check()?;
        }
        *self.accounts.lock().unwrap() = accounts;
        *self.records.lock().unwrap() = records;
        Ok(summary)
    }
    fn records(&self, _q: &RecordQuery) -> Result<Page<GachaRecord>> {
        unimplemented!()
    }
    fn timeline(&self, _a: &AccountId, _p: Option<&PoolTypeId>) -> Result<Vec<GachaRecord>> {
        Ok(self.records.lock().unwrap().clone())
    }
    fn latest_cursor(&self, a: &AccountId, p: &PoolTypeId) -> Result<Option<Cursor>> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .filter(|r| &r.account_id == a && &r.data.pool_type == p)
            .max_by_key(|r| (r.data.pulled_at, r.data.seq_in_sec))
            .map(|r| Cursor {
                ext_id: r.data.ext_id.clone(),
                pulled_at: r.data.pulled_at,
                seq_in_sec: r.data.seq_in_sec,
            }))
    }
    fn backup(&self, _dest: &Path) -> Result<()> {
        unimplemented!()
    }
}

#[derive(Default)]
struct CountingBackup(Mutex<u32>);

impl BackupService for CountingBackup {
    fn snapshot(&self, reason: BackupReason) -> Result<BackupInfo> {
        assert_eq!(reason, BackupReason::Import);
        let mut n = self.0.lock().unwrap();
        *n += 1;
        Ok(BackupInfo {
            name: format!("backup-{n}.db"),
            path: PathBuf::new(),
            created_at: 0,
            size: 0,
        })
    }
}

/// 测试用格式：文件内容就是 `Vec<ImportedAccount>` 的 JSON，以 `TEST` 开头。
struct TestFormat;

impl Converter for TestFormat {
    fn format(&self) -> FormatId {
        FormatId::Gnj
    }
    fn sniff(&self, head: &[u8]) -> bool {
        head.windows(4).any(|w| w == b"TEST")
    }
    fn decode(&self, input: &mut dyn Read) -> Result<Vec<ImportedAccount>> {
        let mut s = String::new();
        input.read_to_string(&mut s)?;
        let start = s.find("TEST").unwrap_or(0) + 4;
        Ok(serde_json::from_str(&s[start..])?)
    }
    fn encode(&self, _data: &[ExportAccount], _out: &mut dyn std::io::Write) -> Result<()> {
        Err(AppError::Invalid("read only".into()))
    }
}

fn registry() -> Arc<PluginRegistry> {
    let mut reg = PluginRegistry::new();
    reg.register_game(Arc::new(TestGame::genshin()))
        .register_converter(Arc::new(TestFormat));
    Arc::new(reg)
}

fn irec(pool: &str, name: &str, rarity: u8, sec: i64, id: Option<&str>) -> ImportedRecord {
    ImportedRecord {
        pool: PoolRef::Official(pool.into()),
        banner_id: None,
        ext_id: id.map(Into::into),
        item_id: None,
        item_name: name.into(),
        item_type: None,
        rarity,
        pulled_at: T0 + sec * 1000,
        seq: None,
        is_pull: true,
        raw: json!({ "name": name }),
        location: format!("list[{sec}]"),
    }
}

fn account(uid: &str, records: Vec<ImportedRecord>) -> ImportedAccount {
    let mut a = ImportedAccount::new("genshin", uid, 480);
    a.records = records;
    a
}

fn file_meta() -> ImportMeta {
    ImportMeta {
        source: ImportSource::File,
        format: Some(FormatId::Gnj),
        file_name: Some("x.json".into()),
        started_at: 0,
    }
}

#[test]
fn normalize_maps_validates_and_reports() {
    let mut bad_time = irec("301", "温迪", 5, 0, Some("9"));
    bad_time.pulled_at = 0;
    let mut a = account(
        "100000001",
        vec![
            irec("301", "温迪", 5, 0, Some("1")),
            irec("400", "可莉", 5, 1, Some("2")),
            irec("200", "冷刃", 3, 2, Some("3")),
            irec("999", "冷刃", 3, 3, Some("4")),
            irec("301", "", 3, 4, Some("5")),
            irec("301", "冷刃", 7, 5, Some("6")),
            bad_time,
        ],
    );
    a.failures
        .push(RecordFailure::new("list[99]", "time not parseable"));
    let n = normalize(&registry(), vec![a], T0 + 10_000);
    assert_eq!(n.imports.len(), 1);
    let pools: Vec<_> = n.imports[0]
        .records
        .iter()
        .map(|r| r.pool_type.as_str())
        .collect();
    assert_eq!(pools, ["character", "character", "standard"]);
    assert_eq!(n.imports[0].records[0].raw_json, r#"{"name":"温迪"}"#);
    let reasons: Vec<_> = n
        .failures
        .iter()
        .map(|f| (f.location.as_str(), f.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        [
            ("list[99]", "time not parseable"),
            ("list[3]", "unknown pool type 999"),
            ("list[4]", "missing item name"),
            ("list[5]", "rarity 7 out of range 1..=5"),
            ("list[0]", "unreasonable time 0"),
        ]
    );
    assert!(n
        .failures
        .iter()
        .all(|f| f.uid == "100000001" && f.game == "genshin"));
}

#[test]
fn normalize_rejects_bad_uid_and_unknown_game() {
    let mut other = ImportedAccount::new("honkai3", "1", 480);
    other.records.push(irec("1", "x", 3, 0, None));
    let n = normalize(&registry(), vec![account("bad uid", vec![]), other], T0);
    assert!(n.imports.is_empty());
    assert_eq!(n.failures[0].reason, "invalid uid for genshin");
    assert_eq!(n.failures[1].reason, "unsupported game honkai3");
}

#[test]
fn normalize_merges_same_account_and_accepts_normalized_pools() {
    let mut r1 = irec("x", "温迪", 5, 0, None);
    r1.pool = PoolRef::Normalized("character".into());
    let mut r2 = irec("x", "温迪", 5, 1, None);
    r2.pool = PoolRef::Normalized("nope".into());
    let n = normalize(
        &registry(),
        vec![
            account("1", vec![r1]),
            account("2", vec![]),
            account("1", vec![r2]),
        ],
        T0 + 10_000,
    );
    assert_eq!(n.imports.len(), 2);
    assert_eq!(n.imports[0].account.uid, "1");
    assert_eq!(n.imports[0].records.len(), 1);
    assert_eq!(n.failures[0].reason, "unknown pool type nope");
}

#[test]
fn seq_in_sec_for_ten_pulls() {
    // 十连同一秒：按官方 ID 数值排序编号，不受文件中的顺序影响；不同卡池类型分别编号
    let ids = [
        "1000000000000000010",
        "999999999999999999",
        "1000000000000000002",
    ];
    let mut recs: Vec<_> = ids
        .iter()
        .map(|id| irec("301", "冷刃", 3, 0, Some(id)))
        .collect();
    recs.push(irec("200", "冷刃", 3, 0, Some("5")));
    let mut with_pos = irec("301", "冷刃", 3, 7, None);
    with_pos.seq = Some(4);
    recs.push(with_pos);
    let n = normalize(&registry(), vec![account("1", recs)], T0 + 10_000);
    let seq: Vec<_> = n.imports[0].records.iter().map(|r| r.seq_in_sec).collect();
    assert_eq!(seq, [2, 0, 1, 0, 4]);
}

fn service(repo: Arc<MemRepo>, backup: Option<Arc<CountingBackup>>) -> ImportService {
    ImportService::new(
        registry(),
        repo,
        backup.map(|b| b as Arc<dyn BackupService>),
    )
}

#[test]
fn import_twice_adds_nothing_second_time() {
    let repo = Arc::new(MemRepo::default());
    let backup = Arc::new(CountingBackup::default());
    let svc = service(repo.clone(), Some(backup.clone()));
    let data = || {
        vec![account(
            "100000001",
            vec![
                irec("301", "冷刃", 3, 0, None),
                irec("301", "冷刃", 3, 0, None), // 十连中同名物品不被误合并
                irec("301", "温迪", 5, 1, Some("7")),
                irec("999", "x", 3, 2, Some("8")),
            ],
        )]
    };
    let first = svc
        .import_accounts(data(), &file_meta(), &NoProgress, &CancelToken::new())
        .unwrap();
    assert_eq!((first.added(), first.skipped(), first.failed()), (3, 0, 1));
    assert!(first.accounts[0].created);
    assert_eq!(first.accounts[0].failed, 1);
    assert_eq!(first.backup.as_deref(), Some("backup-1.db"));
    assert_eq!(first.account_ids(), vec![AccountId::from("acc-0")]);

    let second = svc
        .import_accounts(data(), &file_meta(), &NoProgress, &CancelToken::new())
        .unwrap();
    assert_eq!((second.added(), second.skipped()), (0, 3));
    assert!(!second.accounts[0].created);
    assert_eq!(repo.records.lock().unwrap().len(), 3);
    assert_eq!(*backup.0.lock().unwrap(), 2);
}

#[test]
fn nothing_valid_means_no_backup_and_no_write() {
    let repo = Arc::new(MemRepo::default());
    let backup = Arc::new(CountingBackup::default());
    let svc = service(repo.clone(), Some(backup.clone()));
    let r = svc
        .import_accounts(
            vec![account("bad uid", vec![])],
            &file_meta(),
            &NoProgress,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(r.failed(), 1);
    assert!(r.accounts.is_empty() && r.backup.is_none());
    assert_eq!(*backup.0.lock().unwrap(), 0);
}

#[test]
fn cancel_or_storage_error_leaves_repo_unchanged() {
    let repo = Arc::new(MemRepo::default());
    let svc = service(repo.clone(), None);
    let cancel = CancelToken::new();
    cancel.cancel();
    let data = vec![account("1", vec![irec("301", "温迪", 5, 0, Some("1"))])];
    assert_eq!(
        svc.import_accounts(data.clone(), &file_meta(), &NoProgress, &cancel),
        Err(AppError::Cancelled)
    );
    assert!(repo.records.lock().unwrap().is_empty());

    let failing = Arc::new(MemRepo {
        fail_insert: true,
        ..MemRepo::default()
    });
    let svc = service(failing.clone(), None);
    let err = svc
        .import_accounts(data, &file_meta(), &NoProgress, &CancelToken::new())
        .unwrap_err();
    assert_eq!(err.code(), "storage");
    assert!(failing.accounts.lock().unwrap().is_empty());
}

fn write_test_file(dir: &Path, accounts: &[ImportedAccount]) -> PathBuf {
    let path = dir.join("wish.json");
    let mut f = File::create(&path).unwrap();
    write!(f, "TEST{}", serde_json::to_string(accounts).unwrap()).unwrap();
    path
}

#[test]
fn import_file_preview_and_unknown_format() {
    let dir = tempfile::tempdir().unwrap();
    let accounts = vec![account(
        "1",
        vec![
            irec("301", "温迪", 5, 5, Some("1")),
            irec("200", "冷刃", 3, 2, Some("2")),
        ],
    )];
    let path = write_test_file(dir.path(), &accounts);
    let repo = Arc::new(MemRepo::default());
    let svc = service(repo, None);

    let preview = svc.preview_file(&path).unwrap();
    assert_eq!(preview.format, FormatId::Gnj);
    assert_eq!(preview.accounts[0].count, 2);
    assert_eq!(preview.accounts[0].from, Some(T0 + 2000));
    assert_eq!(preview.accounts[0].to, Some(T0 + 5000));

    let report = svc
        .import_file(&path, &NoProgress, &CancelToken::new())
        .unwrap();
    assert_eq!(report.added(), 2);

    // 格式标记不在文件开头时读整个文件识别
    let late = dir.path().join("late.json");
    std::fs::write(&late, format!("{}TEST[]", " ".repeat(SNIFF_LEN))).unwrap();
    assert!(svc.preview_file(&late).unwrap().accounts.is_empty());
    std::fs::write(&late, " ".repeat(SNIFF_LEN + 1)).unwrap();
    assert_eq!(svc.preview_file(&late), Err(AppError::UnknownFormat));

    let other = dir.path().join("x.txt");
    std::fs::write(&other, "hello").unwrap();
    assert_eq!(svc.preview_file(&other), Err(AppError::UnknownFormat));
    assert_eq!(
        svc.import_file(
            &dir.path().join("missing"),
            &NoProgress,
            &CancelToken::new()
        )
        .unwrap_err()
        .code(),
        "internal"
    );
}

#[derive(Default)]
struct Events(Mutex<Vec<String>>);

impl TaskEvents for Events {
    fn progress(&self, _id: u64, e: ProgressEvent) {
        self.0.lock().unwrap().push(format!("{:?}", e.stage));
    }
    fn done(&self, _id: u64, r: &ImportReport) {
        self.0.lock().unwrap().push(format!("done {}", r.added()));
    }
    fn failed(&self, _id: u64, e: &AppError) {
        self.0.lock().unwrap().push(format!("failed {}", e.code()));
    }
}

#[test]
fn spawned_import_reports_stages_and_respects_busy_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_test_file(
        dir.path(),
        &[account("1", vec![irec("301", "温迪", 5, 0, Some("1"))])],
    );
    let repo = Arc::new(MemRepo::default());
    let svc = service(repo, Some(Arc::new(CountingBackup::default())));
    let tasks = TaskManager::new();
    let events = Arc::new(Events::default());

    let report = svc
        .spawn_import_file(&tasks, events.clone(), path.clone())
        .wait()
        .unwrap();
    assert_eq!(report.added(), 1);
    assert_eq!(
        *events.0.lock().unwrap(),
        ["Parse", "Backup", "Write", "Write", "done 1"]
    );

    // 同一账号已有写入任务时失败
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let (claimed_tx, claimed_rx) = std::sync::mpsc::channel::<()>();
    let blocker = tasks.spawn(events.clone(), move |ctx| {
        let _c = ctx.claim(["genshin:1"])?;
        claimed_tx.send(()).unwrap();
        rx.recv().unwrap();
        Ok(ImportReport::default())
    });
    claimed_rx.recv().unwrap();
    let busy = svc.spawn_import_file(&tasks, events.clone(), path).wait();
    assert_eq!(busy, Err(AppError::Busy("genshin:1".into())));
    tx.send(()).unwrap();
    blocker.wait().unwrap();
}

#[test]
fn spawned_import_only_writes_selected_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_test_file(
        dir.path(),
        &[
            account("1", vec![irec("301", "温迪", 5, 0, Some("1"))]),
            account("2", vec![irec("301", "可莉", 5, 0, Some("2"))]),
        ],
    );
    let repo = Arc::new(MemRepo::default());
    let svc = service(repo, None);
    let tasks = TaskManager::new();
    let events = Arc::new(Events::default());
    let report = svc
        .spawn_import_selected(
            &tasks,
            events,
            path,
            Some(vec![(GameId::from("genshin"), "2".to_owned())]),
        )
        .wait()
        .unwrap();
    assert_eq!(report.accounts.len(), 1);
    assert_eq!(report.accounts[0].uid, "2");
}

#[test]
fn now_is_reasonable() {
    assert!(now_ms() > T0);
    let r = ImportReport::default();
    assert_eq!((r.added(), r.skipped(), r.failed()), (0, 0, 0));
}

#[test]
fn spawned_fetch_uses_since_and_imports() {
    let repo = Arc::new(MemRepo::default());
    let svc = service(repo.clone(), Some(Arc::new(CountingBackup::default())));
    let tasks = TaskManager::new();
    let events = Arc::new(Events::default());
    let game = GameId::from("genshin");

    // 第一次：账号不存在，起点为空
    let report = svc
        .spawn_fetch(
            &tasks,
            events.clone(),
            FetchMode::Incremental,
            ImportSource::Url,
            |_ctx, since| {
                assert!(since.since(&"genshin".into(), "1")?.is_empty());
                Ok(vec![account(
                    "1",
                    vec![
                        irec("301", "温迪", 5, 0, Some("1")),
                        irec("400", "可莉", 5, 1, Some("2")),
                    ],
                )])
            },
        )
        .wait()
        .unwrap();
    assert_eq!(report.added(), 2);

    // 第二次：起点为各卡池类型最新的一条；未知游戏与未知账号为空
    let lookup = svc.since_lookup(FetchMode::Incremental);
    let since = lookup.since(&game, "1").unwrap();
    assert_eq!(since.len(), 1);
    assert_eq!(since["character"].ext_id.as_deref(), Some("2"));
    assert!(lookup.since(&game, "2").unwrap().is_empty());
    assert!(svc
        .since_lookup(FetchMode::Full)
        .since(&game, "1")
        .unwrap()
        .is_empty());
    let other = RepoSince::new(Arc::new(PluginRegistry::new()), repo.clone());
    assert!(other.since(&game, "1").unwrap().is_empty());

    // 拉取失败或被取消时不写入
    let err = svc
        .spawn_fetch(
            &tasks,
            events.clone(),
            FetchMode::Full,
            ImportSource::Url,
            |_ctx, _since| Err(AppError::AuthExpired),
        )
        .wait();
    assert_eq!(err, Err(AppError::AuthExpired));
    let cancelled = svc
        .spawn_fetch(
            &tasks,
            events,
            FetchMode::Full,
            ImportSource::Url,
            |ctx, _since| {
                ctx.cancel_token().cancel();
                Ok(vec![account(
                    "1",
                    vec![irec("301", "迪卢克", 5, 9, Some("9"))],
                )])
            },
        )
        .wait();
    assert_eq!(cancelled, Err(AppError::Cancelled));
    assert_eq!(repo.records.lock().unwrap().len(), 2);
}
