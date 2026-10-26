//! 在游戏数据目录的 Chromium 磁盘缓存中查找抽卡链接（FR-FET-01）。
//!
//! 1. 由外壳提供候选路径：Unity 日志（`output_log.txt` / `Player.log`）、安装目录或数据目录；
//! 2. 从日志中找出数据目录（`.../YuanShen_Data` 等），或直接认出数据目录；
//! 3. 读最新版本的 `webCaches/<版本>/Cache/Cache_Data/data_*`，按出现顺序取出带 `authkey` 的链接。
//!
//! 只读不写；缓存文件被游戏占用时 Windows 上仍可共享读取。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::bytes::Regex;

use super::{HoyoGame, HoyoUrl};

/// 缓存中的 URL：可打印 ASCII，遇到空白、引号或二进制数据即结束。
static URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"https://[!#-&(-;=?-~]+"#).expect("正则合法"));

/// 日志中的数据目录：`X:/.../YuanShen_Data` 等。
fn data_dir_re(game: HoyoGame) -> Regex {
    let names = game.data_folder_names().join("|");
    Regex::new(&format!(r"((?:[A-Za-z]:)?[/\\][^\r\n:*?<>|]*?(?:{names}))")).expect("正则合法")
}

/// 从候选路径找出游戏数据目录，去重，日志中靠后（较新）的在前。
pub fn data_dirs(game: HoyoGame, path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if path.is_file() {
        let Ok(bytes) = fs::read(path) else {
            return out;
        };
        for m in data_dir_re(game).captures_iter(&bytes) {
            let s = String::from_utf8_lossy(&m[1]).into_owned();
            out.push(PathBuf::from(s));
        }
        out.reverse();
    } else if path.join("webCaches").is_dir() {
        out.push(path.to_owned());
    } else {
        for name in game.data_folder_names() {
            let p = path.join(name);
            if p.join("webCaches").is_dir() {
                out.push(p);
            }
        }
    }
    let mut seen = HashSet::new();
    out.retain(|p| p.join("webCaches").is_dir() && seen.insert(p.clone()));
    out
}

/// `webCaches` 下的版本目录名（`2.31.0.0` 等）。
fn version_of(name: &str) -> Option<Vec<u32>> {
    let parts: Option<Vec<u32>> = name.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| (3..=4).contains(&p.len()))
}

/// 最新版本的 `Cache_Data` 目录；旧版游戏没有版本目录。
fn cache_data_dir(data_dir: &Path) -> Option<PathBuf> {
    let web = data_dir.join("webCaches");
    let latest = fs::read_dir(&web)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| version_of(&e.file_name().to_string_lossy()).map(|v| (v, e.path())))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, p)| p);
    let dir = latest.unwrap_or(web).join("Cache").join("Cache_Data");
    dir.is_dir().then_some(dir)
}

/// 数据目录缓存中的抽卡链接，去重，后写入（较新）的在前。
pub fn cache_urls(game: HoyoGame, data_dir: &Path) -> Vec<String> {
    let Some(dir) = cache_data_dir(data_dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("data_"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut urls = Vec::new();
    for f in files {
        let Ok(bytes) = fs::read(&f) else {
            continue;
        };
        for m in URL_RE.find_iter(&bytes) {
            let s = String::from_utf8_lossy(m.as_bytes()).into_owned();
            if HoyoUrl::parse(&s).is_some_and(|u| u.game == game) {
                urls.push(s);
            }
        }
    }
    let mut seen = HashSet::new();
    let mut out: Vec<String> = urls
        .into_iter()
        .rev()
        .filter(|u| seen.insert(u.clone()))
        .collect();
    out.truncate(20);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(key: &str) -> String {
        format!("https://webstatic.mihoyo.com/hk4e/event/e20190909gacha-v3/index.html?authkey_ver=1&authkey={key}&game_biz=hk4e_cn&region=cn_gf01#/log")
    }

    /// 搭一个原神安装目录：日志指向数据目录，缓存里有两条链接。
    fn fixture(root: &Path) -> (PathBuf, PathBuf) {
        let data = root.join("Genshin Impact Game").join("YuanShen_Data");
        let cache = data.join("webCaches/2.31.0.0/Cache/Cache_Data");
        fs::create_dir_all(&cache).unwrap();
        fs::create_dir_all(data.join("webCaches/2.9.0.0/Cache/Cache_Data")).unwrap();
        fs::write(
            data.join("webCaches/2.9.0.0/Cache/Cache_Data/data_2"),
            url("old"),
        )
        .unwrap();
        let mut blob = b"\x00\x01garbage1/0/".to_vec();
        blob.extend(url("first").as_bytes());
        blob.extend(b"\x00\xff\x10https://example.com/x?authkey=1\x00");
        blob.extend(b"1/0/");
        blob.extend(url("second").as_bytes());
        blob.extend(b"\x00");
        blob.extend(url("first").as_bytes());
        blob.extend(b"\x00");
        fs::write(cache.join("data_2"), &blob).unwrap();
        fs::write(cache.join("index"), url("not-a-data-file")).unwrap();
        let log = root.join("output_log.txt");
        let data_str = data.to_string_lossy().replace('\\', "/");
        fs::write(
            &log,
            format!("Warmup file C:/nowhere/YuanShen_Data/x\n[Subsystems] Discovering subsystems at path {data_str}/UnitySubsystems\n"),
        )
        .unwrap();
        (log, data)
    }

    #[test]
    fn finds_latest_urls() {
        let dir = tempfile::tempdir().unwrap();
        let (_, data) = fixture(dir.path());
        let urls = cache_urls(HoyoGame::Genshin, &data);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("authkey=first"));
        assert!(urls[1].contains("authkey=second"));
        assert!(cache_urls(HoyoGame::StarRail, &data).is_empty());
        assert!(cache_urls(HoyoGame::Genshin, dir.path()).is_empty());
    }

    #[test]
    fn locates_data_dir() {
        let dir = tempfile::tempdir().unwrap();
        let (_, data) = fixture(dir.path());
        assert_eq!(data_dirs(HoyoGame::Genshin, &data), vec![data.clone()]);
        let install = data.parent().unwrap();
        assert_eq!(data_dirs(HoyoGame::Genshin, install), vec![data.clone()]);
        assert!(data_dirs(HoyoGame::Zzz, install).is_empty());
        assert!(data_dirs(HoyoGame::Genshin, &dir.path().join("missing")).is_empty());
    }

    #[test]
    fn data_dir_from_log() {
        let re = data_dir_re(HoyoGame::Genshin);
        let caps: Vec<_> = re
            .captures_iter(
                b"Warmup file D:/Games/Genshin Impact Game/YuanShen_Data/StreamingAssets\n",
            )
            .map(|c| String::from_utf8_lossy(&c[1]).into_owned())
            .collect();
        assert_eq!(caps, ["D:/Games/Genshin Impact Game/YuanShen_Data"]);
        // 日志中不存在的目录（C:/nowhere）被跳过
        let dir = tempfile::tempdir().unwrap();
        let (log, data) = fixture(dir.path());
        assert_eq!(data_dirs(HoyoGame::Genshin, &log), vec![data]);
    }

    #[test]
    fn versions() {
        assert_eq!(version_of("2.31.0.0"), Some(vec![2, 31, 0, 0]));
        assert_eq!(version_of("1.0.0"), Some(vec![1, 0, 0]));
        assert_eq!(version_of("Cache"), None);
        assert_eq!(version_of("1.0"), None);
        // 旧版没有版本目录
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("webCaches/Cache/Cache_Data");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("data_2"), url("legacy")).unwrap();
        assert_eq!(cache_urls(HoyoGame::Genshin, dir.path()).len(), 1);
    }
}
