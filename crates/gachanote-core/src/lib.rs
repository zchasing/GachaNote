//! GachaNote 核心层：领域模型、插件 trait、统计引擎与导入导出管线。
//!
//! 本 crate 不依赖任何其他内部 crate，也不依赖 Tauri、SQLite、网络库的具体实现，
//! 只定义 trait 与纯逻辑，供桌面端、后续手机端和服务器端复用。

pub mod redact;

pub use redact::Redacted;
