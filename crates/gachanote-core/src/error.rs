//! 统一错误类型。
//!
//! 所有 crate 的公开接口都返回 [`AppError`]，序列化为 `{ code, message, detail? }`，
//! 前端按 `code` 查 i18n 文案并给出下一步建议（架构文档 7.4）。

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::ids::GameId;

/// 本项目的 `Result` 别名。
pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// 应用错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppError {
    /// 无法识别文件格式。
    #[error("file format not recognized")]
    UnknownFormat,
    /// 文件内容解析失败。`location` 为行号或 JSON 路径。
    #[error("parse error at {location}: {reason}")]
    Parse {
        /// 出错位置。
        location: String,
        /// 原因。
        reason: String,
    },
    /// 找不到抽卡记录链接。
    #[error("url not found")]
    UrlNotFound {
        /// 哪款游戏。
        game: GameId,
    },
    /// authkey / token 过期。
    #[error("auth expired")]
    AuthExpired,
    /// 触发频率限制。
    #[error("rate limited")]
    RateLimited,
    /// 网络错误。
    #[error("network: {0}")]
    Network(String),
    /// 数据库错误。
    #[error("storage: {0}")]
    Storage(String),
    /// 找不到对象（账号、备份等）。
    #[error("not found: {0}")]
    NotFound(String),
    /// 同一账号已有写入任务在进行。
    #[error("busy: another write task is running for {0}")]
    Busy(String),
    /// 输入不合法（如 UID 格式错误、未注册的游戏）。
    #[error("invalid input: {0}")]
    Invalid(String),
    /// 任务被取消，数据未写入。
    #[error("cancelled")]
    Cancelled,
    /// 其他内部错误。
    #[error("internal: {0}")]
    Internal(String),
}

impl AppError {
    /// 前端用来查 i18n 文案的错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownFormat => "unknown_format",
            Self::Parse { .. } => "parse",
            Self::UrlNotFound { .. } => "url_not_found",
            Self::AuthExpired => "auth_expired",
            Self::RateLimited => "rate_limited",
            Self::Network(_) => "network",
            Self::Storage(_) => "storage",
            Self::NotFound(_) => "not_found",
            Self::Busy(_) => "busy",
            Self::Invalid(_) => "invalid",
            Self::Cancelled => "cancelled",
            Self::Internal(_) => "internal",
        }
    }

    /// 只在「显示详情」里展示的附加信息。
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::Parse { location, reason } => Some(format!("{location}: {reason}")),
            Self::UrlNotFound { game } => Some(game.to_string()),
            Self::Network(s)
            | Self::Storage(s)
            | Self::NotFound(s)
            | Self::Busy(s)
            | Self::Invalid(s)
            | Self::Internal(s) => Some(s.clone()),
            Self::UnknownFormat | Self::AuthExpired | Self::RateLimited | Self::Cancelled => None,
        }
    }

    /// 构造解析错误。
    pub fn parse(location: impl Into<String>, reason: impl ToString) -> Self {
        Self::Parse {
            location: location.into(),
            reason: reason.to_string(),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let detail = self.detail();
        let mut s = serializer.serialize_struct("AppError", 3)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        match &detail {
            Some(d) => s.serialize_field("detail", d)?,
            None => s.skip_field("detail")?,
        }
        s.end()
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal(format!("io: {e}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::parse(format!("line {} column {}", e.line(), e.column()), e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_code_message_detail() {
        let e = AppError::parse("line 3", "bad rank_type");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({ "code": "parse", "message": "parse error at line 3: bad rank_type", "detail": "line 3: bad rank_type" })
        );
        assert_eq!(
            serde_json::to_value(AppError::Cancelled).unwrap(),
            json!({ "code": "cancelled", "message": "cancelled" })
        );
    }

    #[test]
    fn every_variant_has_code_and_detail_rule() {
        let all = [
            AppError::UnknownFormat,
            AppError::parse("a", "b"),
            AppError::UrlNotFound {
                game: "wuwa".into(),
            },
            AppError::AuthExpired,
            AppError::RateLimited,
            AppError::Network("n".into()),
            AppError::Storage("s".into()),
            AppError::NotFound("x".into()),
            AppError::Busy("x".into()),
            AppError::Invalid("x".into()),
            AppError::Cancelled,
            AppError::Internal("i".into()),
        ];
        let codes: std::collections::HashSet<_> = all.iter().map(AppError::code).collect();
        assert_eq!(codes.len(), all.len());
        assert_eq!(all[2].detail().as_deref(), Some("wuwa"));
        assert!(all[0].detail().is_none());
        assert!(all[3].detail().is_none());
        assert!(all[4].detail().is_none());
        assert_eq!(all[5].detail().as_deref(), Some("n"));
    }

    #[test]
    fn conversions() {
        let io = std::io::Error::other("boom");
        assert_eq!(AppError::from(io).code(), "internal");
        let json_err = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let e = AppError::from(json_err);
        assert_eq!(e.code(), "parse");
        assert!(e.to_string().contains("line 1"));
    }
}
