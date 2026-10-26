//! tracing 日志：按天滚动写入数据目录下的 `logs/`，保留 14 天。
//!
//! 默认级别发布版为 INFO、开发版为 DEBUG，可用 `RUST_LOG` 覆盖。日志里的 URL、UID、
//! token 必须先包一层 [`gachanote_core::Redacted`]，见架构文档 7.5。

use std::path::Path;
use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

/// 保留的日志文件个数（按天滚动，即 14 天）。
const MAX_LOG_FILES: usize = 14;

const DEFAULT_LEVEL: &str = if cfg!(debug_assertions) {
    "debug"
} else {
    "info"
};

/// 持有后台写日志线程的句柄；被丢弃时会把缓冲区刷到磁盘，所以要放进 Tauri 状态里保持存活。
pub struct LogGuard(#[allow(dead_code)] Mutex<WorkerGuard>);

/// 初始化全局日志。只能调用一次。
pub fn init(log_dir: &Path) -> Result<LogGuard, Box<dyn std::error::Error>> {
    // 目录不存在时 tracing-appender 清理旧文件会报错，先建好
    std::fs::create_dir_all(log_dir)?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("gachanote")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let mut layers = vec![file_layer(writer)];
    if cfg!(debug_assertions) {
        layers.push(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .boxed(),
        );
    }

    tracing_subscriber::registry()
        .with(layers.with_filter(filter()))
        .try_init()?;

    Ok(LogGuard(Mutex::new(guard)))
}

fn filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LEVEL))
}

/// 写文件用的格式：不带颜色，带目标模块名。
fn file_layer<W>(writer: W) -> Box<dyn Layer<Registry> + Send + Sync>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(writer)
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gachanote_core::Redacted;
    use std::io;
    use std::sync::Arc;

    /// 把日志写进内存，便于断言输出内容。
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Buffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Buffer {
        type Writer = Buffer;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn capture(f: impl FnOnce()) -> String {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry().with(file_layer(buffer.clone()));
        tracing::subscriber::with_default(subscriber, f);
        let bytes = buffer.0.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn log_output_redacts_url_query_uid_and_token() {
        let url = Redacted::url(
            "https://hk4e-api.mihoyo.com/gacha_info/api/getGachaLog?authkey=AUTHKEY_SECRET&page=1",
        );
        let uid = Redacted::uid("123456789");
        let token = Redacted::secret("TOKEN_SECRET");

        let out = capture(|| {
            tracing::info!(%url, %uid, ?token, "拉取抽卡记录");
        });

        assert!(out.contains("https://hk4e-api.mihoyo.com/gacha_info/api/getGachaLog"));
        assert!(out.contains("12*****89"));
        assert!(out.contains("Redacted(***)"));
        // 失败消息只给序号，不回显敏感值本身（避免 CodeQL rust/cleartext-logging）
        for (i, secret) in ["AUTHKEY_SECRET", "page=1", "123456789", "TOKEN_SECRET"]
            .iter()
            .enumerate()
        {
            assert!(!out.contains(secret), "日志泄露了第 {i} 个敏感值");
        }
    }

    #[test]
    fn file_log_has_no_ansi_colors() {
        let out = capture(|| tracing::warn!("hello"));
        assert!(out.contains("WARN"));
        assert!(!out.contains('\u{1b}'));
    }

    #[test]
    fn init_creates_log_file_in_dir() {
        let dir = std::env::temp_dir().join(format!("gachanote-log-test-{}", std::process::id()));
        let guard = init(&dir).expect("初始化日志失败");
        tracing::info!("first line");
        drop(guard);

        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().into_string().unwrap();
        assert!(
            name.starts_with("gachanote.") && name.ends_with(".log"),
            "{name}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
