use super::*;
use crate::dto::{TaskDone, TaskFailed};
use std::sync::mpsc;
use std::time::Duration;

/// 收集事件；任务结束时通过通道通知测试线程。
struct Sink {
    events: Mutex<Vec<AppEvent>>,
    finished: Mutex<mpsc::Sender<Result<TaskDone, TaskFailed>>>,
}

impl EventSink for Sink {
    fn emit(&self, event: AppEvent) {
        match &event {
            AppEvent::Done(d) => {
                let _ = lock(&self.finished).send(Ok(d.clone()));
            }
            AppEvent::Failed(f) => {
                let _ = lock(&self.finished).send(Err(f.clone()));
            }
            _ => {}
        }
        lock(&self.events).push(event);
    }
}

struct Fixture {
    state: AppState,
    sink: Arc<Sink>,
    rx: mpsc::Receiver<Result<TaskDone, TaskFailed>>,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(dir.path().join("base"));
        let (tx, rx) = mpsc::channel();
        let sink = Arc::new(Sink {
            events: Mutex::new(Vec::new()),
            finished: Mutex::new(tx),
        });
        let state = AppState::open(paths, sink.clone()).unwrap();
        Self {
            state,
            sink,
            rx,
            dir,
        }
    }

    fn wait(&self) -> Result<TaskDone, TaskFailed> {
        self.rx.recv_timeout(Duration::from_secs(20)).unwrap()
    }

    fn data_changes(&self) -> usize {
        lock(&self.sink.events)
            .iter()
            .filter(|e| matches!(e, AppEvent::DataChanged(_)))
            .count()
    }
}

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/gachanote-formats/tests/samples")
        .join(name)
}

fn genshin_only() -> Option<Vec<AccountKey>> {
    Some(vec![AccountKey {
        game: "genshin".into(),
        uid: "100000001".into(),
    }])
}

#[test]
fn lists_six_games_with_pool_rules() {
    let f = Fixture::new();
    let games = f.state.list_games();
    let ids: Vec<&str> = games.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "genshin",
            "starrail",
            "zzz",
            "arknights",
            "endfield",
            "wuwa"
        ]
    );
    let genshin = &games[0];
    assert!(genshin.can_fetch);
    assert_eq!(genshin.rarity_top, 5);
    let character = &genshin.pools[0];
    assert_eq!(
        (character.id.as_str(), character.hard, character.guarantee),
        ("character", 90, true)
    );
    assert!((character.expected_pulls - 62.3).abs() < 0.5);
    let weapon = genshin.pools.iter().find(|p| p.id == "weapon").unwrap();
    assert_eq!(weapon.path_max, Some(1));
    let ak = &games[3];
    assert!(!ak.can_fetch);
    // 鹰角 token 拉取在 v0.2
    assert_eq!(ak.fetch, None);
    let limited = ak.pools.iter().find(|p| p.id == "limited").unwrap();
    assert_eq!(
        (limited.up, limited.spark, limited.per_banner),
        (crate::dto::UpKind::Fixed, Some(300), true)
    );
    assert!(f.state.list_accounts().unwrap().is_empty());
    assert!(f.state.get_overview().unwrap().is_empty());
}

#[test]
fn preview_import_stats_records_and_export() {
    let f = Fixture::new();
    let path = sample("uigf-v4.json");
    let preview = f.state.preview_file(&path).unwrap();
    assert_eq!(preview.format, "uigf_v4");
    assert_eq!(preview.file_name, "uigf-v4.json");
    assert!(preview.accounts.len() >= 2);
    assert!(preview.accounts.iter().all(|a| !a.exists && a.supported));

    // 只导入勾选的原神账号
    f.state.import_file(path.clone(), genshin_only());
    let done = f.wait().unwrap();
    assert_eq!(done.report.accounts.len(), 1);
    assert_eq!(done.report.added, 52);
    assert!(done.report.backup.is_some());
    assert_eq!(f.data_changes(), 1);

    let accounts = f.state.list_accounts().unwrap();
    assert_eq!(accounts.len(), 1);
    let acc = &accounts[0];
    assert_eq!((acc.game.as_str(), acc.records), ("genshin", 52));
    assert!(acc.last_import_at.is_some());

    // 再次导入：全部重复
    f.state.import_file(path.clone(), genshin_only());
    assert_eq!(f.wait().unwrap().report.skipped, 52);
    assert!(f.state.preview_file(&path).unwrap().accounts[0].exists);

    let stats = f.state.get_stats(&acc.id).unwrap();
    assert_eq!(stats.total_pulls, 52);
    let character = &stats.pools[0];
    assert_eq!(character.pool_type, "character");
    assert_eq!(character.tops[0].item_name, "刻晴");
    assert_eq!(character.tops[0].pulls, 10);
    assert!(character.first_at.is_some() && character.last_at >= character.first_at);
    assert!(stats.top_count >= 1);
    let overview = f.state.get_overview().unwrap();
    assert_eq!(overview.len(), 1);
    assert_eq!(overview[0].stats, stats);

    // 记录列表：最新的在前，五星带当次抽数
    let page = f
        .state
        .query_records(RecordQueryDto {
            account_id: acc.id.clone(),
            pool_types: vec!["character".into()],
            rarities: vec![5],
            from: None,
            to: None,
            name: Some("  ".into()),
            page: 0,
            page_size: 20,
        })
        .unwrap();
    assert!(page.total >= 1);
    let keqing = page.items.iter().find(|r| r.item_name == "刻晴").unwrap();
    assert_eq!(keqing.pulls, Some(10));
    assert!(keqing.up.is_some());
    let all = f
        .state
        .query_records(RecordQueryDto {
            account_id: acc.id.clone(),
            pool_types: vec![],
            rarities: vec![],
            from: None,
            to: None,
            name: None,
            page: 1,
            page_size: 20,
        })
        .unwrap();
    assert_eq!((all.total, all.items.len(), all.page), (52, 20, 1));

    // 导出三种格式；GNJ 再导入不新增
    for (format, name) in [
        (ExportFormat::Gnj, "out.json"),
        (ExportFormat::Csv, "out.csv"),
        (ExportFormat::Uigf, "uigf.json"),
    ] {
        let out = f.dir.path().join(name);
        let r = f
            .state
            .export(std::slice::from_ref(&acc.id), format, &out)
            .unwrap();
        assert_eq!((r.accounts, r.records), (1, 52));
        assert!(out.is_file());
    }
    assert!(!f.dir.path().join("out.gachanote-tmp").exists());
    f.state.import_file(f.dir.path().join("out.json"), None);
    assert_eq!(f.wait().unwrap().report.added, 0);
    assert_eq!(
        f.state
            .export(&[], ExportFormat::Gnj, &f.dir.path().join("x"))
            .unwrap_err()
            .code(),
        "invalid"
    );
    assert_eq!(
        f.state
            .export(
                &["missing".into()],
                ExportFormat::Gnj,
                &f.dir.path().join("x")
            )
            .unwrap_err()
            .code(),
        "not_found"
    );

    // 改名、删除
    f.state.rename_account(&acc.id, Some("  主号 ")).unwrap();
    assert_eq!(
        f.state.list_accounts().unwrap()[0].nickname.as_deref(),
        Some("主号")
    );
    f.state.rename_account(&acc.id, Some("")).unwrap();
    assert_eq!(f.state.list_accounts().unwrap()[0].nickname, None);
    let backups_before = f.state.list_backups().unwrap().len();
    f.state.delete_account(&acc.id).unwrap();
    assert!(f.state.list_accounts().unwrap().is_empty());
    assert!(f.state.list_backups().unwrap().len() > backups_before.min(4));
    assert_eq!(f.state.get_stats(&acc.id).unwrap_err().code(), "not_found");
    assert_eq!(
        f.state.delete_account(&acc.id).unwrap_err().code(),
        "not_found"
    );
}

#[test]
fn import_errors_are_reported_as_failed_tasks() {
    let f = Fixture::new();
    let bad = f.dir.path().join("bad.txt");
    fs::write(&bad, "hello").unwrap();
    assert_eq!(
        f.state.preview_file(&bad).unwrap_err().code(),
        "unknown_format"
    );
    f.state.import_file(bad, None);
    assert_eq!(f.wait().unwrap_err().error.code, "unknown_format");
    assert!(!f.state.cancel_task(9999));
}

#[test]
fn fetch_validates_sources() {
    let f = Fixture::new();
    let wuwa = "https://aki-gm-resources.aki-game.com/aki/gacha/index.html#/record?svr_id=x&player_id=100000001&lang=zh-Hans&gacha_id=100001&gacha_type=1&svr_area=cn&record_id=abc&resources_id=r";
    let hoyo = "https://public-operation-hk4e.mihoyo.com/gacha_info/api/getGachaLog?authkey_ver=1&authkey=SECRET&lang=zh-cn&game_biz=hk4e_cn";
    let pasted = |url: &str| UrlSource::Pasted { url: url.into() };
    let code = |game: &str, src| {
        f.state
            .fetch_by_url(game, src, FetchModeDto::Incremental)
            .unwrap_err()
            .code()
    };
    assert_eq!(code("genshin", pasted("not a url")), "invalid");
    assert_eq!(code("starrail", pasted(hoyo)), "invalid");
    assert_eq!(code("genshin", pasted(wuwa)), "invalid");
    assert_eq!(
        code("genshin", UrlSource::Discovered { id: "src-1".into() }),
        "not_found"
    );
    assert_eq!(code("arknights", pasted(hoyo)), "invalid");
    assert_eq!(code("nope", pasted(hoyo)), "invalid");
    let err = tauri::async_runtime::block_on(f.state.discover_url("arknights")).unwrap_err();
    assert_eq!(err.code(), "invalid");
    if !cfg!(windows) {
        // Linux CI 上没有游戏目录
        let err = tauri::async_runtime::block_on(f.state.discover_url("genshin")).unwrap_err();
        assert_eq!(err.code(), "url_not_found");
    }
}

#[test]
fn settings_round_trip_and_tolerate_old_values() {
    let f = Fixture::new();
    assert_eq!(f.state.get_settings().unwrap(), Settings::default());
    let mut s = Settings {
        language: "en".into(),
        theme: "dark".into(),
        proxy: Some("http://127.0.0.1:7890".into()),
        sidebar_games: vec!["wuwa".into()],
        meta_auto_update: false,
        ..Settings::default()
    };
    s.last_accounts.insert("genshin".into(), "acc".into());
    f.state.set_settings(&s).unwrap();
    assert_eq!(f.state.get_settings().unwrap(), s);
    f.state
        .store
        .set_setting(
            SETTINGS_KEY,
            &serde_json::json!({ "theme": "light", "sidebarGames": 3 }),
        )
        .unwrap();
    let got = f.state.get_settings().unwrap();
    assert_eq!(got.theme, "light");
    assert!(got.sidebar_games.is_empty());
    assert_eq!(got.language, "system");
    // 无效代理不会让客户端创建失败
    let _ = http_client(Some("::not a proxy::"));
    let _ = http_client(Some("http://127.0.0.1:7890"));
}

#[test]
fn backups_restore_and_data_dir() {
    let f = Fixture::new();
    f.state.import_file(sample("uigf-v4.json"), genshin_only());
    let done = f.wait().unwrap();
    let manual = f.state.create_backup().unwrap();
    assert!(manual.size > 0);
    f.state
        .delete_account(&done.report.accounts[0].account_id)
        .unwrap();
    f.state.restore_backup(&manual.name).unwrap();
    assert_eq!(f.state.list_accounts().unwrap().len(), 1);
    assert_eq!(
        f.state
            .restore_backup("gachanote-19990101-000000.db")
            .unwrap_err()
            .code(),
        "not_found"
    );

    let info = f.state.app_info();
    assert!(info.db_size > 0);
    assert_eq!(info.data_dir, info.default_data_dir);
    assert_eq!(f.state.dir(DirKind::Logs), f.state.paths.logs());
    assert_eq!(f.state.dir(DirKind::Data), f.state.paths.data);
    assert_eq!(f.state.dir(DirKind::Backups), f.state.paths.backups());

    // 搬到新目录：数据库与备份被复制，重启后读到新位置
    let new_dir = f.dir.path().join("moved");
    assert_eq!(
        f.state
            .set_data_dir(Path::new("relative"))
            .unwrap_err()
            .code(),
        "invalid"
    );
    f.state.set_data_dir(&new_dir).unwrap();
    assert!(new_dir.join(DB_FILE).is_file());
    assert!(new_dir.join("backups").read_dir().unwrap().count() > 0);
    let resolved = Paths::resolve(f.state.paths.base.clone());
    assert_eq!(resolved.data, new_dir);
    let reopened = SqliteStore::open(&resolved.db()).unwrap();
    assert_eq!(reopened.accounts(None).unwrap().len(), 1);
    assert_eq!(
        f.state.set_data_dir(&new_dir).unwrap_err().code(),
        "invalid"
    );
    // 改回默认目录：删除位置文件
    f.state.set_data_dir(&f.state.paths.base.clone()).unwrap();
    assert_eq!(
        Paths::resolve(f.state.paths.base.clone()).data,
        f.state.paths.base
    );
}

#[test]
fn metadata_info_and_outcomes() {
    let f = Fixture::new();
    let info = f.state.meta_info();
    assert_eq!(info.origin, "bundled");
    assert!(!info.version.is_empty());
    let r = f.state.apply_meta_outcome(UpdateOutcome::UpToDate {
        version: "1".into(),
    });
    assert_eq!(r.outcome, "upToDate");
    let r = f.state.apply_meta_outcome(UpdateOutcome::AppTooOld {
        version: "9".into(),
        min_app_version: "9.0.0".into(),
    });
    assert_eq!(
        (r.outcome.as_str(), r.min_app_version.as_deref()),
        ("appTooOld", Some("9.0.0"))
    );
    let r = f.state.apply_meta_outcome(UpdateOutcome::Updated {
        version: "2".into(),
    });
    assert_eq!(r.outcome, "updated");
    assert!(lock(&f.sink.events)
        .iter()
        .any(|e| matches!(e, AppEvent::MetaUpdated(v) if v == "2")));
}

#[test]
fn paths_ignore_broken_location_file() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    fs::write(base.join(LOCATION_FILE), "{ broken").unwrap();
    assert_eq!(Paths::resolve(base.clone()).data, base);
    fs::write(base.join(LOCATION_FILE), r#"{"dataDir":"relative/dir"}"#).unwrap();
    assert_eq!(Paths::resolve(base.clone()).data, base);
    let p = Paths::resolve(base.clone());
    assert_eq!(p.db(), base.join(DB_FILE));
    assert_eq!(p.metadata(), base.join("metadata"));
}

/// 性能基准（T-40），默认不跑。
mod perf;
