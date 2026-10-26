use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

/// 一套最小的包：一个游戏文件 + manifest。
fn pack_files(version: &str, min_app: &str) -> BTreeMap<String, Vec<u8>> {
    let game = br#"{"game":"genshin","items":[],"standard":["Diluc"],"banners":[]}"#.to_vec();
    let manifest = serde_json::to_vec(&Manifest {
        version: version.into(),
        min_app_version: Some(min_app.into()),
        files: vec![ManifestFile {
            path: "genshin.json".into(),
            sha256: sha256_hex(&game),
        }],
    })
    .unwrap();
    BTreeMap::from([
        ("manifest.json".to_owned(), manifest),
        ("genshin.json".to_owned(), game),
    ])
}

async fn serve(files: &BTreeMap<String, Vec<u8>>) -> MockServer {
    let server = MockServer::start().await;
    for (p, bytes) in files {
        Mock::given(path(format!("/metadata/{p}")))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()))
            .mount(&server)
            .await;
    }
    server
}

fn source(server: &MockServer) -> Box<dyn PackSource> {
    Box::new(HttpSource::new(
        "mock",
        format!("{}/metadata/", server.uri()),
        reqwest::Client::new(),
    ))
}

#[test]
fn bundled_pack_is_valid() {
    let set = MetaSet::bundled();
    assert_eq!(set.origin, PackOrigin::Bundled);
    assert_eq!(set.games.len(), 6);
    let genshin = set.game(&GameId::GENSHIN.into());
    assert!(genshin.banners.len() > 100);
    // 名单展开后同时含 ID 与名称
    assert!(genshin.standard.iter().any(|s| s == "迪卢克"));
    assert!(genshin.standard.iter().any(|s| s == "Diluc"));
    assert!(set
        .game(&GameId::WUWA.into())
        .standard
        .contains(&"维里奈".to_owned()));
    assert!(set.game(&"unknown".into()).banners.is_empty());
    for (id, pack) in &set.packs {
        assert_eq!(pack.game, id.as_str());
    }
    assert_eq!(default_sources(&reqwest::Client::new()).len(), 2);
}

#[test]
fn rejects_tampered_files() {
    let mut files = pack_files("1", "0.1.0");
    files.insert("genshin.json".into(), b"{}".to_vec());
    let e = MetaSet::from_files(&files, PackOrigin::Cache).unwrap_err();
    assert!(e.to_string().contains("sha256"), "{e}");
    files.remove("genshin.json");
    assert!(MetaSet::from_files(&files, PackOrigin::Cache).is_err());
    files.remove("manifest.json");
    assert!(MetaSet::from_files(&files, PackOrigin::Cache).is_err());
}

#[test]
fn load_prefers_newer_valid_cache() {
    let dir = tempfile::tempdir().unwrap();
    let store = MetaStore::new(dir.path().join("metadata"));
    assert_eq!(store.dir(), dir.path().join("metadata"));
    // 没有缓存
    assert_eq!(store.load().origin, PackOrigin::Bundled);

    // 比内置包新的缓存
    store.write_cache(&pack_files("9999.1.1", "0.1.0")).unwrap();
    let set = store.load();
    assert_eq!((set.origin, set.version()), (PackOrigin::Cache, "9999.1.1"));

    // 比内置包旧的缓存
    store.write_cache(&pack_files("2000.1.1", "0.1.0")).unwrap();
    assert_eq!(store.load().origin, PackOrigin::Bundled);

    // 损坏的缓存
    store.write_cache(&pack_files("9999.1.1", "0.1.0")).unwrap();
    fs::write(store.dir().join("genshin.json"), "{}").unwrap();
    assert_eq!(store.load().origin, PackOrigin::Bundled);
}

#[tokio::test]
async fn updates_from_first_good_source() {
    let dir = tempfile::tempdir().unwrap();
    let store = MetaStore::new(dir.path().join("metadata"));
    let good = pack_files("9999.1.1", "0.1.0");

    // 第一个来源哈希不符，第二个来源不可达，第三个来源正确
    let mut tampered = good.clone();
    tampered.insert("genshin.json".into(), b"{\"game\":\"genshin\"}".to_vec());
    let bad = serve(&tampered).await;
    let empty = MockServer::start().await;
    let ok = serve(&good).await;
    let sources = vec![source(&bad), source(&empty), source(&ok)];
    let outcome = store.update(&sources).await.unwrap();
    assert_eq!(
        outcome,
        UpdateOutcome::Updated {
            version: "9999.1.1".into()
        }
    );
    assert_eq!(store.load().origin, PackOrigin::Cache);
    // 临时目录已清理
    let leftovers: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(leftovers.len(), 1);

    // 再检查一次：已是最新
    let outcome = store.update(&[source(&ok)]).await.unwrap();
    assert_eq!(
        outcome,
        UpdateOutcome::UpToDate {
            version: "9999.1.1".into()
        }
    );
}

#[tokio::test]
async fn keeps_old_pack_when_update_fails() {
    let dir = tempfile::tempdir().unwrap();
    let store = MetaStore::new(dir.path().join("metadata"));

    // 都失败：返回最后一个错误，不写缓存
    let empty = MockServer::start().await;
    let e = store.update(&[source(&empty)]).await.unwrap_err();
    assert_eq!(e.code(), "network");
    assert!(!store.dir().exists());
    assert_eq!(store.update(&[]).await.unwrap_err().code(), "network");

    // 需要更新的应用
    let server = serve(&pack_files("9999.1.1", "999.0.0")).await;
    let outcome = store.update(&[source(&server)]).await.unwrap();
    assert_eq!(
        outcome,
        UpdateOutcome::AppTooOld {
            version: "9999.1.1".into(),
            min_app_version: "999.0.0".into()
        }
    );
    assert!(!store.dir().exists());

    // manifest 中的路径不能跳出目录
    let mut files = pack_files("9999.1.1", "0.1.0");
    let mut manifest: Manifest = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest.files[0].path = "../evil.json".into();
    files.insert(
        "manifest.json".into(),
        serde_json::to_vec(&manifest).unwrap(),
    );
    let server = serve(&files).await;
    let e = store.update(&[source(&server)]).await.unwrap_err();
    assert!(e.to_string().contains("invalid path"), "{e}");
    assert!(!dir.path().join("evil.json").exists());
}
