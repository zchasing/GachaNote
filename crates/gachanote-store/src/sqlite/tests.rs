use super::*;
use crate::backup::BackupManager;
use gachanote_core::model::ImportSource;
use gachanote_core::FormatId;
use std::sync::Arc;

const T0: i64 = 1_700_000_000_000;

fn rec(pool: &str, name: &str, rarity: u8, sec: i64, ext: Option<&str>, seq: u32) -> NewRecord {
    NewRecord {
        pool_type: pool.into(),
        banner_id: None,
        ext_id: ext.map(Into::into),
        item_id: Some("1".into()),
        item_name: name.into(),
        item_type: Some("武器".into()),
        rarity,
        pulled_at: T0 + sec * 1000,
        seq_in_sec: seq,
        is_pull: true,
        raw_json: format!(r#"{{"name":"{name}"}}"#),
    }
}

fn meta() -> ImportMeta {
    ImportMeta {
        source: ImportSource::File,
        format: Some(FormatId::UigfV4),
        file_name: Some("a.json".into()),
        started_at: T0,
    }
}

fn imp(uid: &str, records: Vec<NewRecord>) -> AccountImport {
    AccountImport {
        account: NewAccount::new("genshin", uid),
        records,
    }
}

fn sample() -> Vec<NewRecord> {
    vec![
        // 十连同一秒的同名物品：自然键靠 seq_in_sec 区分
        rec("character", "冷刃", 3, 0, None, 0),
        rec("character", "冷刃", 3, 0, None, 1),
        rec("character", "温迪", 5, 1, Some("1001"), 0),
        rec("standard", "刻晴", 5, 2, Some("1002"), 0),
        rec("standard", "100% 黎明_神剑", 3, 3, Some("1003"), 0),
    ]
}

fn insert(store: &SqliteStore, data: &[AccountImport]) -> Result<InsertSummary> {
    store.insert_records(&meta(), data, &CancelToken::new())
}

#[test]
fn open_file_uses_wal_and_latest_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/gachanote.db");
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    let mode: String = store
        .conn()
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let fk: i64 = store
        .conn()
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1);
    drop(store);
    // 再次打开不重复迁移，也不调用备份
    let called = std::cell::Cell::new(false);
    let backup = |_: &SqliteStore| -> Result<()> {
        called.set(true);
        Ok(())
    };
    SqliteStore::open_with_backup(&path, Some(&backup)).unwrap();
    assert!(!called.get());
}

#[test]
fn refuses_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.db");
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    let err = SqliteStore::open(&path).err().unwrap();
    assert!(err.to_string().contains("newer"));
}

#[test]
fn account_upsert_rename_delete() {
    let store = SqliteStore::open_in_memory().unwrap();
    let a = store
        .upsert_account(&NewAccount::new("genshin", "100000001"))
        .unwrap();
    let mut with_region = NewAccount::new("genshin", "100000001");
    with_region.region = Some("cn_gf01".into());
    with_region.nickname = Some("大号".into());
    let b = store.upsert_account(&with_region).unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(b.region.as_deref(), Some("cn_gf01"));
    // 已有的字段不被覆盖
    with_region.nickname = Some("改名".into());
    assert_eq!(
        store
            .upsert_account(&with_region)
            .unwrap()
            .nickname
            .as_deref(),
        Some("大号")
    );

    store
        .upsert_account(&NewAccount::new("starrail", "100000001"))
        .unwrap();
    assert_eq!(store.accounts(None).unwrap().len(), 2);
    assert_eq!(store.accounts(Some(&"starrail".into())).unwrap().len(), 1);

    store.rename_account(&a.id, Some("小号")).unwrap();
    assert_eq!(
        store.account(&a.id).unwrap().unwrap().nickname.as_deref(),
        Some("小号")
    );
    store.rename_account(&a.id, None).unwrap();
    assert_eq!(store.account(&a.id).unwrap().unwrap().nickname, None);
    assert_eq!(
        store.rename_account(&"x".into(), None).unwrap_err().code(),
        "not_found"
    );

    store.delete_account(&a.id).unwrap();
    assert!(store.account(&a.id).unwrap().is_none());
    assert_eq!(store.delete_account(&a.id).unwrap_err().code(), "not_found");
}

#[test]
fn repeated_import_adds_zero() {
    let store = SqliteStore::open_in_memory().unwrap();
    let s1 = insert(&store, &[imp("1", sample())]).unwrap();
    assert_eq!((s1.added(), s1.skipped()), (5, 0));
    assert!(s1.accounts[0].created);
    let s2 = insert(&store, &[imp("1", sample())]).unwrap();
    assert_eq!((s2.added(), s2.skipped()), (0, 5));
    assert!(!s2.accounts[0].created);
    let id = &s1.accounts[0].account.id;
    assert_eq!(store.timeline(id, None).unwrap().len(), 5);
    assert_eq!(store.batch_count(id).unwrap(), 2);
    let (added, skipped, finished): (i64, i64, Option<i64>) = store
        .conn()
        .query_row(
            "SELECT added, skipped, finished_at FROM import_batch WHERE id = ?1",
            [s2.accounts[0].batch_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((added, skipped), (0, 5));
    assert!(finished.is_some());

    // 官方 ID 相同即视为重复，即使其他字段不同
    let mut changed = rec("character", "可莉", 5, 9, Some("1001"), 0);
    changed.rarity = 4;
    let s3 = insert(&store, &[imp("1", vec![changed]), imp("2", sample())]).unwrap();
    assert_eq!(
        s3.accounts
            .iter()
            .map(|a| (a.added, a.skipped))
            .collect::<Vec<_>>(),
        [(0, 1), (5, 0)]
    );
}

#[test]
fn failure_midway_rolls_back_everything() {
    let store = SqliteStore::open_in_memory().unwrap();
    insert(&store, &[imp("1", sample())]).unwrap();
    store
        .conn()
        .execute_batch(
            "CREATE TRIGGER boom BEFORE INSERT ON gacha_record WHEN NEW.item_name = 'BOOM' \
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    let mut recs = vec![rec("character", "新的", 3, 50, None, 0)];
    recs.push(rec("character", "BOOM", 3, 51, None, 0));
    let err = insert(
        &store,
        &[
            imp("3", vec![rec("character", "x", 3, 60, None, 0)]),
            imp("1", recs),
        ],
    )
    .unwrap_err();
    assert_eq!(err.code(), "storage");
    assert_eq!(store.accounts(None).unwrap().len(), 1);
    let total: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM gacha_record", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 5);
    let batches: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM import_batch", [], |r| r.get(0))
        .unwrap();
    assert_eq!(batches, 1);
}

#[test]
fn cancelled_insert_writes_nothing() {
    let store = SqliteStore::open_in_memory().unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let err = store
        .insert_records(&meta(), &[imp("1", sample())], &cancel)
        .unwrap_err();
    assert_eq!(err, AppError::Cancelled);
    assert!(store.accounts(None).unwrap().is_empty());
}

#[test]
fn query_filters_paging_and_order() {
    let store = SqliteStore::open_in_memory().unwrap();
    let s = insert(&store, &[imp("1", sample())]).unwrap();
    let id = s.accounts[0].account.id.clone();

    let mut q = RecordQuery::new(id.clone(), 2);
    let p0 = store.records(&q).unwrap();
    assert_eq!((p0.total, p0.items.len(), p0.page_size), (5, 2, 2));
    assert_eq!(p0.items[0].data.item_name, "100% 黎明_神剑");
    q.page = 2;
    let p2 = store.records(&q).unwrap();
    assert_eq!(p2.items.len(), 1);
    assert_eq!(
        (
            p2.items[0].data.item_name.as_str(),
            p2.items[0].data.seq_in_sec
        ),
        ("冷刃", 0)
    );

    q.page = 0;
    q.page_size = 100;
    q.order = SortOrder::OldestFirst;
    q.pool_types = vec!["character".into()];
    let names: Vec<_> = store
        .records(&q)
        .unwrap()
        .items
        .into_iter()
        .map(|r| r.data.item_name)
        .collect();
    assert_eq!(names, ["冷刃", "冷刃", "温迪"]);

    q.pool_types.clear();
    q.rarities = vec![5];
    assert_eq!(store.records(&q).unwrap().total, 2);
    q.rarities.clear();
    q.from = Some(T0 + 1000);
    q.to = Some(T0 + 3000);
    assert_eq!(store.records(&q).unwrap().total, 2);
    q.from = None;
    q.to = None;
    // LIKE 的通配符按字面匹配
    q.name = Some("0% 黎".into());
    assert_eq!(store.records(&q).unwrap().total, 1);
    q.name = Some("_".into());
    assert_eq!(store.records(&q).unwrap().total, 1);
    q.name = Some("%".into());
    assert_eq!(store.records(&q).unwrap().total, 1);
    q.name = Some(String::new());
    assert_eq!(store.records(&q).unwrap().total, 5);
    q.page_size = 0;
    assert_eq!(store.records(&q).unwrap().page_size, 1);

    let r = &store.records(&RecordQuery::new(id, 1)).unwrap().items[0];
    assert_eq!(r.data.raw_json, r#"{"name":"100% 黎明_神剑"}"#);
    assert_eq!(r.data.item_type.as_deref(), Some("武器"));
    assert!(r.batch_id.is_some());
}

#[test]
fn timeline_and_latest_cursor() {
    let store = SqliteStore::open_in_memory().unwrap();
    let mut recs = sample();
    recs.reverse();
    let s = insert(&store, &[imp("1", recs)]).unwrap();
    let id = s.accounts[0].account.id.clone();
    let tl = store.timeline(&id, Some(&"character".into())).unwrap();
    let seq: Vec<_> = tl
        .iter()
        .map(|r| (r.data.item_name.as_str(), r.data.seq_in_sec))
        .collect();
    assert_eq!(seq, [("冷刃", 0), ("冷刃", 1), ("温迪", 0)]);

    let c = store
        .latest_cursor(&id, &"character".into())
        .unwrap()
        .unwrap();
    assert_eq!(c.ext_id.as_deref(), Some("1001"));
    assert_eq!(c.pulled_at, T0 + 1000);
    assert!(store
        .latest_cursor(&id, &"weapon".into())
        .unwrap()
        .is_none());
}

#[test]
fn delete_account_cascades() {
    let store = Arc::new(SqliteStore::open_in_memory().unwrap());
    let s = insert(&store, &[imp("1", sample()), imp("2", sample())]).unwrap();
    let id = s.accounts[0].account.id.clone();
    let dir = tempfile::tempdir().unwrap();
    let backups = BackupManager::new(dir.path(), 5, store.clone());
    store.delete_account_with_backup(&id, &backups).unwrap();
    assert_eq!(backups.list().unwrap().len(), 1);
    assert!(store.timeline(&id, None).unwrap().is_empty());
    assert_eq!(store.batch_count(&id).unwrap(), 0);
    assert_eq!(
        store
            .timeline(&s.accounts[1].account.id, None)
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        store
            .delete_account_with_backup(&id, &backups)
            .unwrap_err()
            .code(),
        "not_found"
    );
}

#[test]
fn settings_round_trip() {
    let store = SqliteStore::open_in_memory().unwrap();
    assert_eq!(store.setting("theme").unwrap(), None);
    store
        .set_setting("theme", &serde_json::json!("dark"))
        .unwrap();
    store
        .set_setting("theme", &serde_json::json!("light"))
        .unwrap();
    assert_eq!(
        store.setting("theme").unwrap(),
        Some(serde_json::json!("light"))
    );
    store
        .conn()
        .execute("INSERT INTO setting VALUES ('bad', '{')", [])
        .unwrap();
    assert_eq!(store.setting("bad").unwrap_err().code(), "storage");
}

#[test]
fn backup_keeps_five_and_restores() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(&dir.path().join("gachanote.db")).unwrap());
    let backups = BackupManager::new(
        dir.path().join("backups"),
        crate::DEFAULT_KEEP,
        store.clone(),
    );
    assert!(backups.list().unwrap().is_empty());
    let s = insert(&store, &[imp("1", sample())]).unwrap();
    let id = s.accounts[0].account.id.clone();

    let first = backups.snapshot(BackupReason::Import).unwrap();
    assert!(first.size > 0);
    let mut names = vec![first.name.clone()];
    for _ in 0..5 {
        names.push(backups.snapshot(BackupReason::Manual).unwrap().name);
    }
    // 第 6 次备份后最旧的一份被删
    let listed: Vec<_> = backups
        .list()
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(listed.len(), 5);
    assert!(!listed.contains(&first.name));
    let mut expected = names[1..].to_vec();
    expected.reverse();
    assert_eq!(listed, expected);
    std::fs::write(backups.dir().join("notes.txt"), "x").unwrap();
    assert_eq!(backups.list().unwrap().len(), 5);

    // 恢复后数据与备份时一致
    let before = store.timeline(&id, None).unwrap();
    insert(
        &store,
        &[imp(
            "1",
            vec![rec("character", "新", 3, 99, Some("2000"), 0)],
        )],
    )
    .unwrap();
    store
        .delete_account(
            &store
                .upsert_account(&NewAccount::new("zzz", "9"))
                .unwrap()
                .id,
        )
        .unwrap();
    assert_eq!(store.timeline(&id, None).unwrap().len(), 6);
    backups.restore(&listed[0]).unwrap();
    assert_eq!(store.timeline(&id, None).unwrap(), before);
    assert_eq!(store.accounts(None).unwrap().len(), 1);
    // 恢复后仍可写入（外键、迁移状态正常）
    insert(
        &store,
        &[imp(
            "1",
            vec![rec("character", "新", 3, 99, Some("2000"), 0)],
        )],
    )
    .unwrap();

    assert_eq!(backups.restore("../evil.db").unwrap_err().code(), "invalid");
    assert_eq!(
        backups
            .restore("gachanote-20000101-000000.db")
            .unwrap_err()
            .code(),
        "not_found"
    );
}

#[test]
fn account_activity_counts_records_and_last_import() {
    let store = SqliteStore::open_in_memory().unwrap();
    assert!(store.account_activity().unwrap().is_empty());
    let empty = store.upsert_account(&NewAccount::new("genshin", "2")).unwrap();
    let s = insert(&store, &[imp("1", sample())]).unwrap();
    let act = store.account_activity().unwrap();
    assert_eq!(act.len(), 2);
    assert_eq!(act[0].account_id, empty.id);
    assert_eq!((act[0].records, act[0].last_import_at), (0, None));
    assert_eq!(act[1].account_id, s.accounts[0].account.id);
    assert_eq!(act[1].records, sample().len() as u64);
    assert!(act[1].last_import_at.is_some_and(|t| t >= T0));
}
