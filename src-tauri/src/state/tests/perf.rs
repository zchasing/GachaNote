//! 性能基准（T-40），对应 SRS 的 NFR-01–03 与 T-35 的翻页指标。
//!
//! 计时在调试构建下没有意义，全部标为 `#[ignore]`，在 release 下单独运行：
//! `cargo test -p gachanote-app --release --locked -- --ignored --test-threads=1`。
//! CI 的「性能基准」任务会跑这一组，超出阈值即失败。

use super::*;
use gachanote_core::model::{AccountImport, ImportMeta, NewAccount, NewRecord};
use gachanote_core::task::CancelToken;
use std::time::Instant;

/// 合成记录：三个卡池轮流，约每 70 抽一个 5 星、每 10 抽一个 4 星。
fn synthetic(n: i64) -> impl Iterator<Item = (i64, &'static str, u8)> {
    let pools = ["character", "weapon", "standard"];
    (0..n).map(move |i| {
        let rarity = if i % 70 == 69 {
            5
        } else if i % 10 == 9 {
            4
        } else {
            3
        };
        (i, pools[(i % 3) as usize], rarity)
    })
}

/// 直接写入数据库（不经过文件解码），返回账号 ID。
fn insert_direct(state: &AppState, n: i64) -> String {
    let t0 = 1_700_000_000_000_i64;
    let records: Vec<NewRecord> = synthetic(n)
        .map(|(i, pool, rarity)| NewRecord {
            pool_type: pool.into(),
            banner_id: None,
            ext_id: Some(format!("{}", 1_000_000 + i)),
            item_id: None,
            item_name: format!("物品{}", i % 97),
            item_type: Some("武器".into()),
            rarity,
            pulled_at: t0 + i * 1000,
            seq_in_sec: 0,
            is_pull: true,
            raw_json: "{}".into(),
        })
        .collect();
    let summary = state
        .store
        .insert_records(
            &ImportMeta {
                source: ImportSource::File,
                format: None,
                file_name: None,
                started_at: t0,
            },
            &[AccountImport {
                account: NewAccount::new("genshin", "100000001"),
                records,
            }],
            &CancelToken::new(),
        )
        .unwrap();
    summary.accounts[0].account.id.to_string()
}

/// 写一个 n 条记录的原神 UIGF v4 文件，每 10 秒一抽。
fn write_uigf(path: &Path, n: i64) {
    let gacha_types = ["301", "302", "200"];
    let mut out = BufWriter::new(fs::File::create(path).unwrap());
    write!(
        out,
        r#"{{"info":{{"export_timestamp":1704100000,"export_app":"bench","export_app_version":"1","version":"v4.0"}},"hk4e":[{{"uid":"100000001","timezone":8,"lang":"zh-cn","list":["#
    )
    .unwrap();
    for (i, _, rarity) in synthetic(n) {
        let secs = i * 10;
        let (day, rest) = (secs / 86_400, secs % 86_400);
        let gacha_type = gacha_types[(i % 3) as usize];
        if i > 0 {
            out.write_all(b",").unwrap();
        }
        write!(
            out,
            r#"{{"uigf_gacha_type":"{gacha_type}","gacha_type":"{gacha_type}","item_id":"{item}","count":"1","time":"2024-01-{:02} {:02}:{:02}:{:02}","name":"物品{}","item_type":"武器","rank_type":"{rarity}","id":"{}"}}"#,
            day + 1,
            rest / 3600,
            rest % 3600 / 60,
            rest % 60,
            i % 97,
            1_704_081_600_000_000_000_i64 + i,
            item = 10_000 + i % 97,
        )
        .unwrap();
    }
    out.write_all(b"]}]}").unwrap();
    out.flush().unwrap();
}

/// NFR-02 导入 10 万条 ≤ 10 秒；NFR-01 的后端部分：重新打开应用到总览数据就绪 ≤ 2 秒。
#[test]
#[ignore]
fn import_100k_under_10s_and_cold_start_under_2s() {
    let f = Fixture::new();
    let file = f.dir.path().join("bench-100k.json");
    write_uigf(&file, 100_000);

    // 和界面一样走完整链路：识别 → 预览 → 后台任务（备份、解码、规范化、单事务写入）
    let start = Instant::now();
    let preview = f.state.preview_file(&file).unwrap();
    assert_eq!(preview.accounts[0].count, 100_000);
    f.state.import_file(file.clone(), None);
    let done =
        f.rx.recv_timeout(Duration::from_secs(120))
            .unwrap()
            .unwrap_or_else(|e| panic!("导入失败：{:?}", e.error));
    let import = start.elapsed();
    assert_eq!(done.report.added, 100_000);
    eprintln!("导入 10 万条：{import:?}");

    // 冷启动：关掉再打开数据库，加载插件与元数据包，算出总览
    let paths = Paths::resolve(f.dir.path().join("base"));
    drop(f.state);
    let (tx, _rx) = mpsc::channel();
    let sink = Arc::new(Sink {
        events: Mutex::new(Vec::new()),
        finished: Mutex::new(tx),
    });
    let start = Instant::now();
    let state = AppState::open(paths, sink).unwrap();
    let games = state.list_games();
    let overview = state.get_overview().unwrap();
    let cold = start.elapsed();
    assert_eq!(games.len(), 6);
    assert_eq!(overview[0].stats.total_pulls, 100_000);
    eprintln!("冷启动到总览数据就绪（后端，10 万条）：{cold:?}");

    assert!(import < Duration::from_secs(10), "导入耗时 {import:?}");
    assert!(cold < Duration::from_secs(2), "冷启动耗时 {cold:?}");
}

/// NFR-03 单账号 5 万条记录时统计重算 ≤ 300 ms（读时间线 + 统计 + 每条记录的标注）。
#[test]
#[ignore]
fn stats_recompute_under_300ms_at_50k() {
    let f = Fixture::new();
    let id = insert_direct(&f.state, 50_000);
    let mut worst = Duration::ZERO;
    for _ in 0..5 {
        f.state.invalidate(&[]);
        let start = Instant::now();
        let stats = f.state.get_stats(&id).unwrap();
        worst = worst.max(start.elapsed());
        assert_eq!(stats.total_pulls, 50_000);
    }
    eprintln!("5 万条统计重算最慢一次：{worst:?}");
    assert!(worst < Duration::from_millis(300), "统计重算耗时 {worst:?}");
}

/// T-35：10 万条记录时翻页查询 < 200 ms。
#[test]
#[ignore]
fn records_page_under_200ms_at_100k() {
    let f = Fixture::new();
    let account_id = insert_direct(&f.state, 100_000);
    let query = |page: u32, rarities: Vec<u8>, name: Option<&str>| RecordQueryDto {
        account_id: account_id.clone(),
        pool_types: vec![],
        rarities,
        from: None,
        to: None,
        name: name.map(Into::into),
        page,
        page_size: 50,
    };

    // 第一次查询要算整个账号的当次抽数与 UP 标记，之后走缓存
    let cold = Instant::now();
    f.state.query_records(query(0, vec![], None)).unwrap();
    let cold = cold.elapsed();

    let mut worst = Duration::ZERO;
    for q in [
        query(0, vec![], None),
        query(1000, vec![], None),
        query(1999, vec![], None),
        query(10, vec![5], None),
        query(0, vec![], Some("物品4")),
    ] {
        let start = Instant::now();
        let label = format!(
            "page {} rarities {:?} name {:?}",
            q.page, q.rarities, q.name
        );
        let page = f.state.query_records(q).unwrap();
        eprintln!("{label}: {:?}", start.elapsed());
        worst = worst.max(start.elapsed());
        assert!(!page.items.is_empty());
    }
    eprintln!("100k records: first query {cold:?}, worst cached page {worst:?}");
    assert!(worst < Duration::from_millis(200), "worst page {worst:?}");
}
