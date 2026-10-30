//! M1 验收：原神 UIGF 样例能导入并算出正确的三项核心指标。
//!
//! 走完整链路：识别格式 → 解码 → 规范化 → 备份 → SQLite 单事务写入 → 读出时间线 → 统计引擎。
//! 样例的角色池序列为：10 抽歪（刻晴）→ 5 抽大保底（温迪，400 池）→ 20 抽不歪（可莉）→ 垫 7 抽。

use std::path::PathBuf;
use std::sync::Arc;

use gachanote_core::import::ImportService;
use gachanote_core::meta::{Banner, GameMeta};
use gachanote_core::plugin::{GameInfo, GamePlugin, PluginRegistry, Vendor};
use gachanote_core::repo::RecordRepository;
use gachanote_core::rules::{PityCurve, PityScope, PoolRule, UpRule};
use gachanote_core::stats::{account_stats, LossRate};
use gachanote_core::task::{CancelToken, NoProgress};
use gachanote_core::{AppError, FormatId, GameId, PoolTypeId};
use gachanote_store::{BackupManager, SqliteStore};

/// 最小的原神插件（完整插件在 M2 的 T-20）。
struct Genshin(Vec<PoolRule>);

impl Genshin {
    fn new() -> Self {
        let curve = PityCurve {
            base: 0.006,
            soft_start: Some(74),
            step: 0.06,
            hard: 90,
        };
        let rule = |pool: &str, up| PoolRule {
            pool_type: pool.into(),
            since: None,
            rarity_top: 5,
            curve,
            up,
            extras: vec![],
            pity_scope: PityScope::SharedByType,
            achievements: Default::default(),
        };
        Self(vec![
            rule(
                "character",
                UpRule::FiftyFifty {
                    rate: 0.5,
                    guarantee_next: true,
                },
            ),
            rule("standard", UpRule::None),
        ])
    }
}

impl GamePlugin for Genshin {
    fn id(&self) -> GameId {
        GameId::GENSHIN.into()
    }
    fn info(&self) -> GameInfo {
        GameInfo {
            id: self.id(),
            vendor: Vendor::Hoyoverse,
            theme_color: "#C8A464".into(),
            pool_types: vec!["character".into(), "standard".into()],
            fetch: None,
            formats: vec![FormatId::UigfV4],
        }
    }
    fn rules(&self) -> &[PoolRule] {
        &self.0
    }
    fn pool_type(&self, official: &str) -> Option<PoolTypeId> {
        match official {
            "301" | "400" => Some("character".into()),
            "200" => Some("standard".into()),
            _ => None,
        }
    }
}

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../gachanote-formats/tests/samples/uigf-v4.json")
}

fn meta() -> GameMeta {
    GameMeta {
        game: Some("genshin".into()),
        banners: vec![Banner {
            id: "5.3-1".into(),
            pool_type: "character".into(),
            name: "test".into(),
            start: 1_704_000_000_000,
            end: 1_705_000_000_000,
            up_top: vec!["温迪".into(), "可莉".into()],
            up_second: vec![],
        }],
        standard: vec!["刻晴".into(), "迪卢克".into()],
    }
}

struct Env {
    _dir: tempfile::TempDir,
    store: Arc<SqliteStore>,
    backups: Arc<BackupManager>,
    service: ImportService,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(&dir.path().join("gachanote.db")).unwrap());
    let backups = Arc::new(BackupManager::new(
        dir.path().join("backups"),
        5,
        store.clone(),
    ));
    let mut registry = PluginRegistry::new();
    registry.register_game(Arc::new(Genshin::new()));
    for c in gachanote_formats::all_converters() {
        registry.register_converter(c);
    }
    let service = ImportService::new(Arc::new(registry), store.clone(), Some(backups.clone()));
    Env {
        _dir: dir,
        store,
        backups,
        service,
    }
}

#[test]
fn import_uigf_and_compute_core_metrics() {
    let env = env();
    let report = env
        .service
        .import_file(&sample(), &NoProgress, &CancelToken::new())
        .unwrap();
    assert_eq!(report.accounts.len(), 1);
    let acc = &report.accounts[0];
    assert_eq!(
        (acc.uid.as_str(), acc.created, acc.added, acc.skipped),
        ("100000001", true, 52, 0)
    );
    // 崩铁、绝区零没有注册插件：整个账号进入失败列表，不影响原神
    let failed: Vec<_> = report
        .failures
        .iter()
        .map(|f| (f.game.as_str(), f.reason.as_str()))
        .collect();
    assert_eq!(
        failed,
        [
            ("starrail", "unsupported game starrail"),
            ("zzz", "unsupported game zzz")
        ]
    );
    assert!(report.backup.is_some());

    let records = env.store.timeline(&acc.account_id, None).unwrap();
    // 十连同一秒：按官方 ID 编出 0–9 的同秒序号
    let ten: Vec<_> = records
        .iter()
        .filter(|r| r.data.pool_type == "standard")
        .map(|r| r.data.seq_in_sec)
        .collect();
    assert_eq!(ten, (0..10).collect::<Vec<_>>());

    let plugin = Genshin::new();
    let stats = account_stats(&plugin, &meta(), &records);
    let character = &stats.pools[0];
    assert_eq!(character.pool_type, "character");
    // 当前保底：垫了 7 抽，小保底
    assert_eq!(character.pity.since_top, 7);
    assert_eq!(character.pity.remaining_to_hard, 83);
    assert!(!character.pity.guaranteed);
    // 平均出 UP 抽数：(10 + 5 + 20) / 2
    assert_eq!(character.avg_pulls_per_up, Some(17.5));
    // 小保底歪率：1 / 2（大保底的温迪不计入分母）
    assert_eq!(character.loss, Some(LossRate { lost: 1, total: 2 }));
    assert_eq!(character.total_pulls, 42);

    let standard = &stats.pools[1];
    assert_eq!(
        (standard.total_pulls, standard.pity.since_top, standard.loss),
        (10, 6, None)
    );
    assert_eq!(stats.total_pulls, 52);
    assert_eq!(stats.rarity_counts.get(&5), Some(&4));
}

#[test]
fn reimport_adds_nothing_and_cancel_changes_nothing() {
    let env = env();
    env.service
        .import_file(&sample(), &NoProgress, &CancelToken::new())
        .unwrap();
    let again = env
        .service
        .import_file(&sample(), &NoProgress, &CancelToken::new())
        .unwrap();
    assert_eq!((again.added(), again.skipped()), (0, 52));
    assert!(!again.accounts[0].created);
    assert_eq!(env.backups.list().unwrap().len(), 2);

    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        env.service.import_file(&sample(), &NoProgress, &cancel),
        Err(AppError::Cancelled)
    );
    let id = &again.accounts[0].account_id;
    assert_eq!(env.store.timeline(id, None).unwrap().len(), 52);
    assert_eq!(env.store.batch_count(id).unwrap(), 2);
}

#[test]
fn export_after_import_round_trips() {
    use gachanote_core::exchange::ExportAccount;
    use gachanote_core::plugin::Converter;

    let env = env();
    let report = env
        .service
        .import_file(&sample(), &NoProgress, &CancelToken::new())
        .unwrap();
    let id = &report.accounts[0].account_id;
    let account = env.store.account(id).unwrap().unwrap();
    let records = env.store.timeline(id, None).unwrap();
    let mut out = Vec::new();
    gachanote_formats::UigfV4
        .encode(&[ExportAccount { account, records }], &mut out)
        .unwrap();
    let path = env._dir.path().join("export.json");
    std::fs::write(&path, out).unwrap();

    // 导出的文件再导入同一个库：全部判为重复
    let again = env
        .service
        .import_file(&path, &NoProgress, &CancelToken::new())
        .unwrap();
    assert_eq!((again.added(), again.skipped()), (0, 52));
    let preview = env.service.preview_file(&path).unwrap();
    assert_eq!(preview.format, FormatId::UigfV4);
}
