//! 敏感信息脱敏。
//!
//! 日志里的 URL、UID、token 一律包一层 [`Redacted`] 再输出，由 `Debug` / `Display`
//! 实现保证脱敏，而不是靠人工注意（见架构文档 7.5）：
//!
//! - URL 只保留协议、host 与 path，去掉用户信息、查询参数和片段；
//! - UID 只保留首尾各 2 位，例如 `12****89`；
//! - token、authkey 等密钥永不输出原文。
//!
//! ```
//! use gachanote_core::Redacted;
//!
//! let url = Redacted::url("https://example.com/gacha?authkey=abc&page=1");
//! assert_eq!(url.to_string(), "https://example.com/gacha");
//!
//! let uid = Redacted::uid("123456789");
//! assert_eq!(uid.to_string(), "12*****89");
//!
//! let token = Redacted::secret("very-secret-token");
//! assert_eq!(format!("{token:?}"), "Redacted(***)");
//! ```

use std::fmt;
use std::marker::PhantomData;

/// 脱敏方式。每种方式是一个零大小的标记类型，决定 [`Redacted`] 如何输出。
pub trait RedactKind {
    /// 把原文按本方式脱敏后写入 `f`。
    fn write(raw: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

/// 密钥（token、authkey、cookie 等）：任何情况下都只输出 `***`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Secret {}

/// 游戏 UID：只保留首尾各 2 位，中间每个字符替换为 `*`；不超过 4 位时全部替换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Uid {}

/// URL：只保留协议、host（含端口）与 path。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Url {}

const MASK: &str = "***";

impl RedactKind for Secret {
    fn write(_raw: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(MASK)
    }
}

impl RedactKind for Uid {
    fn write(raw: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = raw.chars().count();
        if len <= 4 {
            return f.write_str(&"*".repeat(len));
        }
        let head: String = raw.chars().take(2).collect();
        let tail: String = raw.chars().skip(len - 2).collect();
        write!(f, "{head}{}{tail}", "*".repeat(len - 4))
    }
}

impl RedactKind for Url {
    fn write(raw: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 片段和查询参数之后的内容全部丢弃
        let raw = raw.split(['#', '?']).next().unwrap_or_default();
        let (scheme, rest) = match raw.split_once("://") {
            Some((scheme, rest)) => (Some(scheme), rest),
            None => (None, raw),
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => rest.split_at(i),
            None => (rest, ""),
        };
        // 去掉 user:password@
        let host = authority.rsplit('@').next().unwrap_or_default();
        if let Some(scheme) = scheme {
            write!(f, "{scheme}://")?;
        }
        write!(f, "{host}{path}")
    }
}

/// 脱敏包装：`Debug` 和 `Display` 都只输出脱敏后的内容，需要原文时调用 [`Redacted::expose`]。
///
/// 默认方式为 [`Secret`]，即完全隐藏。
pub struct Redacted<T, K: RedactKind = Secret> {
    value: T,
    kind: PhantomData<K>,
}

impl<T: AsRef<str>> Redacted<T, Secret> {
    /// 包装一个密钥。
    pub fn secret(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: AsRef<str>> Redacted<T, Uid> {
    /// 包装一个 UID。
    pub fn uid(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: AsRef<str>> Redacted<T, Url> {
    /// 包装一个 URL。
    pub fn url(value: T) -> Self {
        Self::new(value)
    }
}

impl<T: AsRef<str>, K: RedactKind> Redacted<T, K> {
    /// 按 `K` 指定的方式包装。
    pub fn new(value: T) -> Self {
        Self {
            value,
            kind: PhantomData,
        }
    }

    /// 取出原文。只在真正需要原文的地方（发请求、写数据库）使用，不要传给日志。
    pub fn expose(&self) -> &T {
        &self.value
    }

    /// 拆出原文。
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: AsRef<str>, K: RedactKind> fmt::Display for Redacted<T, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        K::write(self.value.as_ref(), f)
    }
}

impl<T: AsRef<str>, K: RedactKind> fmt::Debug for Redacted<T, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Redacted(")?;
        K::write(self.value.as_ref(), f)?;
        f.write_str(")")
    }
}

impl<T: Clone, K: RedactKind> Clone for Redacted<T, K> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            kind: PhantomData,
        }
    }
}

impl<T: PartialEq, K: RedactKind> PartialEq for Redacted<T, K> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Eq, K: RedactKind> Eq for Redacted<T, K> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_drops_query_and_fragment() {
        let url = Redacted::url(
            "https://public-operation-hk4e.mihoyo.com/gacha_info/api/getGachaLog?authkey=SECRET&gacha_type=301#top",
        );
        let shown = url.to_string();
        assert_eq!(
            shown,
            "https://public-operation-hk4e.mihoyo.com/gacha_info/api/getGachaLog"
        );
        assert!(!format!("{url:?}").contains("SECRET"));
    }

    #[test]
    fn url_drops_userinfo_and_keeps_port() {
        let url = Redacted::url("http://user:pass@127.0.0.1:8080/api?x=1");
        assert_eq!(url.to_string(), "http://127.0.0.1:8080/api");
    }

    #[test]
    fn url_without_scheme_or_path() {
        assert_eq!(Redacted::url("example.com").to_string(), "example.com");
        assert_eq!(
            Redacted::url("example.com/a/b?token=1").to_string(),
            "example.com/a/b"
        );
        assert_eq!(Redacted::url("?authkey=1").to_string(), "");
    }

    #[test]
    fn uid_keeps_first_and_last_two() {
        assert_eq!(Redacted::uid("12345689").to_string(), "12****89");
        assert_eq!(Redacted::uid("100000001").to_string(), "10*****01");
        assert_eq!(format!("{:?}", Redacted::uid("12345")), "Redacted(12*45)");
    }

    #[test]
    fn short_uid_is_fully_masked() {
        assert_eq!(Redacted::uid("1234").to_string(), "****");
        assert_eq!(Redacted::uid("").to_string(), "");
    }

    #[test]
    fn uid_counts_chars_not_bytes() {
        assert_eq!(Redacted::uid("测试账号名字").to_string(), "测试**名字");
    }

    #[test]
    fn secret_never_shows_value() {
        let token = Redacted::secret(String::from("abcdef"));
        assert_eq!(token.to_string(), "***");
        assert_eq!(format!("{token:?}"), "Redacted(***)");
        assert_eq!(format!("{token:#?}"), "Redacted(***)");
    }

    #[test]
    fn redacted_in_struct_debug() {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Request {
            url: Redacted<&'static str, Url>,
            uid: Redacted<&'static str, Uid>,
            authkey: Redacted<&'static str>,
        }
        let req = Request {
            url: Redacted::url("https://a.com/b?authkey=K"),
            uid: Redacted::uid("123456789"),
            authkey: Redacted::secret("K"),
        };
        let shown = format!("{req:?}");
        assert_eq!(
            shown,
            "Request { url: Redacted(https://a.com/b), uid: Redacted(12*****89), authkey: Redacted(***) }"
        );
    }

    #[test]
    fn expose_and_into_inner_return_raw_value() {
        let token = Redacted::secret(String::from("raw"));
        assert_eq!(token.expose(), "raw");
        let copy = token.clone();
        assert_eq!(copy, token);
        assert_eq!(token.into_inner(), "raw");
    }
}
