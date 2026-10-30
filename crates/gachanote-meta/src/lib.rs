//! GachaNote 卡池元数据包：格式、manifest 校验、三级下载兜底与本地缓存（T-25，FR-META-01–03）。
//!
//! 启动时 [`MetaStore::load`] 读本地缓存，缓存缺失、损坏或比内置包旧时用安装包内置的版本，
//! 所以断网也能统计。[`MetaStore::update`] 依次尝试 raw.githubusercontent.com、jsDelivr：
//! 远程版本更新时下载全部文件，逐个校验 SHA-256，全部通过才整体替换缓存；
//! 哈希不符或下载失败时换下一个来源，都失败时保留旧包。

pub mod icons;
pub mod pack;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use gachanote_core::meta::GameMeta;
use gachanote_core::{AppError, GameId, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use icons::{icon_path, IconStore, ICON_MIME};
pub use pack::{cmp_version, is_sha256, GamePack, Manifest, ManifestFile, PackItem};

/// 本应用版本。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 安装包内置的元数据包（仓库 `metadata/` 目录，编译时嵌入）。
pub const BUNDLED: &[(&str, &[u8])] = &[
    (
        "manifest.json",
        include_bytes!("../../../metadata/manifest.json"),
    ),
    (
        "genshin.json",
        include_bytes!("../../../metadata/genshin.json"),
    ),
    (
        "starrail.json",
        include_bytes!("../../../metadata/starrail.json"),
    ),
    ("zzz.json", include_bytes!("../../../metadata/zzz.json")),
    (
        "arknights.json",
        include_bytes!("../../../metadata/arknights.json"),
    ),
    (
        "endfield.json",
        include_bytes!("../../../metadata/endfield.json"),
    ),
    ("wuwa.json", include_bytes!("../../../metadata/wuwa.json")),
];

/// 小写十六进制 SHA-256。
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 包从哪里来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackOrigin {
    /// 安装包内置。
    Bundled,
    /// 本地缓存（之前下载的）。
    Cache,
}

/// 已加载的元数据包。
#[derive(Debug, Clone, PartialEq)]
pub struct MetaSet {
    /// manifest。
    pub manifest: Manifest,
    /// 来源。
    pub origin: PackOrigin,
    /// 各游戏的原始包（物品名称等）。
    pub packs: BTreeMap<GameId, GamePack>,
    /// 各游戏的统计用元数据。
    pub games: BTreeMap<GameId, GameMeta>,
}

impl MetaSet {
    /// 包版本。
    pub fn version(&self) -> &str {
        &self.manifest.version
    }

    /// 某游戏的元数据；没有时为空（UP 判定退化为「未知」）。
    pub fn game(&self, id: &GameId) -> GameMeta {
        self.games.get(id).cloned().unwrap_or_default()
    }

    /// 校验并解析一整套文件：manifest 列出的每个文件都要存在、SHA-256 一致、内容合法。
    pub fn from_files(files: &BTreeMap<String, Vec<u8>>, origin: PackOrigin) -> Result<Self> {
        let manifest_bytes = files
            .get("manifest.json")
            .ok_or_else(|| AppError::parse("manifest.json", "missing"))?;
        let manifest: Manifest = serde_json::from_slice(manifest_bytes)?;
        let mut set = Self {
            manifest,
            origin,
            packs: BTreeMap::new(),
            games: BTreeMap::new(),
        };
        for f in &set.manifest.files {
            let bytes = files
                .get(&f.path)
                .ok_or_else(|| AppError::parse(&f.path, "missing"))?;
            verify(&f.path, bytes, &f.sha256)?;
            if !f.path.ends_with(".json") {
                continue;
            }
            let pack = GamePack::parse(bytes).map_err(|e| AppError::parse(&f.path, e))?;
            let id = GameId::from(pack.game.as_str());
            set.games.insert(id.clone(), pack.to_game_meta()?);
            set.packs.insert(id, pack);
        }
        Ok(set)
    }

    /// 内置包。内容在编译期固定并有测试保证合法。
    pub fn bundled() -> Self {
        let files = BUNDLED
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.to_vec()))
            .collect();
        Self::from_files(&files, PackOrigin::Bundled).expect("内置元数据包合法")
    }
}

fn verify(path: &str, bytes: &[u8], expected: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(AppError::parse(path, "sha256 mismatch"))
    }
}

/// 元数据包的下载来源。
#[async_trait]
pub trait PackSource: Send + Sync {
    /// 来源名称（日志用）。
    fn name(&self) -> &str;
    /// 取 `metadata/` 下的一个文件。
    async fn get(&self, path: &str) -> Result<Vec<u8>>;
}

/// 通过 HTTP 下载：`<base>/<path>`。
pub struct HttpSource {
    name: String,
    base: String,
    client: reqwest::Client,
}

impl HttpSource {
    /// 新建。`base` 不带结尾的 `/`。
    pub fn new(name: impl Into<String>, base: impl Into<String>, client: reqwest::Client) -> Self {
        Self {
            name: name.into(),
            base: base.into().trim_end_matches('/').to_owned(),
            client,
        }
    }
}

#[async_trait]
impl PackSource for HttpSource {
    fn name(&self) -> &str {
        &self.name
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get(format!("{}/{path}", self.base))
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(AppError::Network(format!("http status {}", resp.status())));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;
        Ok(bytes.to_vec())
    }
}

/// 默认来源：raw.githubusercontent.com → jsDelivr（FR-META-02）。
pub fn default_sources(client: &reqwest::Client) -> Vec<Box<dyn PackSource>> {
    vec![
        Box::new(HttpSource::new(
            "github",
            "https://raw.githubusercontent.com/zchasing/GachaNote/main/metadata",
            client.clone(),
        )),
        Box::new(HttpSource::new(
            "jsdelivr",
            "https://cdn.jsdelivr.net/gh/zchasing/GachaNote@main/metadata",
            client.clone(),
        )),
    ]
}

/// 一次检查更新的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// 已是最新。
    UpToDate {
        /// 当前版本。
        version: String,
    },
    /// 已下载并替换缓存（之后发 `meta://updated`）。
    Updated {
        /// 新版本。
        version: String,
    },
    /// 远程包需要更新的应用版本，暂不使用。
    AppTooOld {
        /// 远程包版本。
        version: String,
        /// 所需应用版本。
        min_app_version: String,
    },
}

/// 本地缓存目录（`%APPDATA%/GachaNote/metadata/`）。
pub struct MetaStore {
    dir: PathBuf,
}

impl MetaStore {
    /// 使用给定的缓存目录。
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 缓存目录。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn read_cache(&self) -> Result<MetaSet> {
        let manifest = fs::read(self.dir.join("manifest.json"))?;
        let parsed: Manifest = serde_json::from_slice(&manifest)?;
        let mut files = BTreeMap::new();
        files.insert("manifest.json".to_owned(), manifest);
        for f in &parsed.files {
            files.insert(f.path.clone(), fs::read(self.dir.join(&f.path))?);
        }
        MetaSet::from_files(&files, PackOrigin::Cache)
    }

    /// 加载当前可用的包：缓存合法且不比内置包旧时用缓存，否则用内置包。不会失败。
    pub fn load(&self) -> MetaSet {
        let bundled = MetaSet::bundled();
        match self.read_cache() {
            Ok(cache) if cmp_version(cache.version(), bundled.version()).is_ge() => cache,
            Ok(_) => bundled,
            Err(e) => {
                if self.dir.join("manifest.json").exists() {
                    tracing::warn!(error = %e, "元数据缓存不可用，改用内置包");
                }
                bundled
            }
        }
    }

    /// 检查并下载新版本。依次尝试各来源；都失败时返回最后一个错误，缓存保持不变。
    pub async fn update(&self, sources: &[Box<dyn PackSource>]) -> Result<UpdateOutcome> {
        let current = self.load();
        let mut last_err = AppError::Network("no metadata source".into());
        for src in sources {
            match self.try_source(src.as_ref(), current.version()).await {
                Ok(outcome) => return Ok(outcome),
                Err(e) => {
                    tracing::warn!(source = src.name(), error = %e, "元数据下载失败，尝试下一个来源");
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    async fn try_source(&self, src: &dyn PackSource, current: &str) -> Result<UpdateOutcome> {
        let manifest_bytes = src.get("manifest.json").await?;
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
        if cmp_version(&manifest.version, current).is_le() {
            return Ok(UpdateOutcome::UpToDate {
                version: current.to_owned(),
            });
        }
        if let Some(min) = &manifest.min_app_version {
            if cmp_version(min, APP_VERSION).is_gt() {
                return Ok(UpdateOutcome::AppTooOld {
                    version: manifest.version,
                    min_app_version: min.clone(),
                });
            }
        }
        let mut files = BTreeMap::new();
        files.insert("manifest.json".to_owned(), manifest_bytes);
        for f in &manifest.files {
            if f.path.contains("..") || f.path.starts_with('/') || f.path.contains('\\') {
                return Err(AppError::parse(&f.path, "invalid path"));
            }
            let bytes = src.get(&f.path).await?;
            verify(&f.path, &bytes, &f.sha256)?;
            files.insert(f.path.clone(), bytes);
        }
        MetaSet::from_files(&files, PackOrigin::Cache)?;
        self.write_cache(&files)?;
        Ok(UpdateOutcome::Updated {
            version: manifest.version,
        })
    }

    /// 先写到临时目录，再整体换掉旧缓存，避免半新半旧。
    fn write_cache(&self, files: &BTreeMap<String, Vec<u8>>) -> Result<()> {
        let parent = self.dir.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let name = self
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "metadata".into());
        let tmp = parent.join(format!(".{name}.tmp"));
        let old = parent.join(format!(".{name}.old"));
        let _ = fs::remove_dir_all(&tmp);
        for (path, bytes) in files {
            let p = tmp.join(path);
            if let Some(dir) = p.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(p, bytes)?;
        }
        let _ = fs::remove_dir_all(&old);
        if self.dir.exists() {
            fs::rename(&self.dir, &old)?;
        }
        fs::rename(&tmp, &self.dir)?;
        let _ = fs::remove_dir_all(&old);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
