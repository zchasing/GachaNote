//! 物品头像（T-60）：按需下载、校验、缓存。
//!
//! 头像不进安装包，也不列在 manifest 里：游戏包里每个物品的 `icon` 字段就是图标文件的
//! SHA-256，游戏包本身已由 manifest 校验，所以图标也间接受校验。文件按内容寻址，
//! 存放在 `icons/<game>/<sha256>.webp`，内容变了文件名也变，缓存不会过期。
//! 下载来源与数据包相同（raw.githubusercontent.com → jsDelivr）。

use std::fs;
use std::path::PathBuf;

use gachanote_core::{AppError, Result};

use crate::pack::is_sha256;
use crate::{sha256_hex, PackSource};

/// 头像图片的 MIME 类型。
pub const ICON_MIME: &str = "image/webp";

/// 图标文件相对 `metadata/` 的路径。
pub fn icon_path(game: &str, sha256: &str) -> String {
    format!("icons/{game}/{sha256}.webp")
}

fn valid_game(game: &str) -> bool {
    !game.is_empty() && game.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// 头像的本地缓存（`%APPDATA%/GachaNote/icons/`）。
pub struct IconStore {
    dir: PathBuf,
}

impl IconStore {
    /// 使用给定的缓存目录。
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn file(&self, game: &str, sha256: &str) -> PathBuf {
        self.dir.join(game).join(format!("{sha256}.webp"))
    }

    fn check(game: &str, sha256: &str) -> Result<()> {
        // 参数来自前端的图片地址，先校验格式，避免拼出目录外的路径
        if valid_game(game) && is_sha256(sha256) {
            Ok(())
        } else {
            Err(AppError::Invalid(format!("invalid icon {game}/{sha256}")))
        }
    }

    /// 已缓存且内容完好的图标；缺失或损坏时为 `None`。
    pub fn cached(&self, game: &str, sha256: &str) -> Option<Vec<u8>> {
        Self::check(game, sha256).ok()?;
        let bytes = fs::read(self.file(game, sha256)).ok()?;
        (sha256_hex(&bytes) == sha256).then_some(bytes)
    }

    /// 取图标：先读缓存，没有时依次从各来源下载，校验通过后写入缓存。
    pub async fn get(
        &self,
        game: &str,
        sha256: &str,
        sources: &[Box<dyn PackSource>],
    ) -> Result<Vec<u8>> {
        Self::check(game, sha256)?;
        if let Some(bytes) = self.cached(game, sha256) {
            return Ok(bytes);
        }
        let path = icon_path(game, sha256);
        let mut last_err = AppError::Network("no icon source".into());
        for src in sources {
            match src.get(&path).await {
                Ok(bytes) if sha256_hex(&bytes) == sha256 => {
                    if let Err(e) = self.write(game, sha256, &bytes) {
                        tracing::warn!(error = %e, "写入头像缓存失败");
                    }
                    return Ok(bytes);
                }
                Ok(_) => last_err = AppError::parse(&path, "sha256 mismatch"),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    fn write(&self, game: &str, sha256: &str, bytes: &[u8]) -> Result<()> {
        let file = self.file(game, sha256);
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        // 先写临时文件再改名，并发请求同一图标时不会读到半个文件
        let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &file)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;

    struct Fixed {
        bytes: Vec<u8>,
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl PackSource for Fixed {
        fn name(&self) -> &str {
            "fixed"
        }
        async fn get(&self, path: &str) -> Result<Vec<u8>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(path.starts_with("icons/genshin/") && path.ends_with(".webp"));
            Ok(self.bytes.clone())
        }
    }

    struct Down;

    #[async_trait]
    impl PackSource for Down {
        fn name(&self) -> &str {
            "down"
        }
        async fn get(&self, _: &str) -> Result<Vec<u8>> {
            Err(AppError::Network("offline".into()))
        }
    }

    #[tokio::test]
    async fn downloads_verifies_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let store = IconStore::new(dir.path());
        let png = b"RIFF....WEBPVP8 ".to_vec();
        let sha = sha256_hex(&png);
        let calls = Arc::new(AtomicUsize::new(0));
        let good: Vec<Box<dyn PackSource>> = vec![
            Box::new(Down),
            Box::new(Fixed {
                bytes: b"tampered".to_vec(),
                calls: Arc::clone(&calls),
            }),
            Box::new(Fixed {
                bytes: png.clone(),
                calls: Arc::clone(&calls),
            }),
        ];
        assert!(store.cached("genshin", &sha).is_none());
        assert_eq!(store.get("genshin", &sha, &good).await.unwrap(), png);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        // 第二次直接读缓存
        assert_eq!(store.get("genshin", &sha, &good).await.unwrap(), png);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(store.cached("genshin", &sha).unwrap(), png);

        // 缓存损坏时重新下载
        fs::write(dir.path().join("genshin").join(format!("{sha}.webp")), b"x").unwrap();
        assert!(store.cached("genshin", &sha).is_none());
        assert_eq!(store.get("genshin", &sha, &good).await.unwrap(), png);
    }

    #[tokio::test]
    async fn rejects_bad_input_and_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = IconStore::new(dir.path());
        let sha = "0".repeat(64);
        assert_eq!(
            store.get("../x", &sha, &[]).await.unwrap_err().code(),
            "invalid"
        );
        assert_eq!(
            store
                .get("genshin", "../../etc", &[])
                .await
                .unwrap_err()
                .code(),
            "invalid"
        );
        assert!(store.cached("Genshin", &sha).is_none());
        let calls = Arc::new(AtomicUsize::new(0));
        let bad: Vec<Box<dyn PackSource>> = vec![Box::new(Fixed {
            bytes: b"not it".to_vec(),
            calls,
        })];
        let e = store.get("genshin", &sha, &bad).await.unwrap_err();
        assert!(e.to_string().contains("sha256"), "{e}");
        assert_eq!(
            store
                .get("genshin", &sha, &[Box::new(Down)])
                .await
                .unwrap_err()
                .code(),
            "network"
        );
        assert_eq!(icon_path("zzz", "ab"), "icons/zzz/ab.webp");
    }
}
