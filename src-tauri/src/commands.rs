//! 暴露给前端的 Tauri 命令。签名由 tauri-specta 导出到 `ui/src/bindings.ts`。

use serde::Serialize;
use specta::Type;

/// `ping` 的返回值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Pong {
    /// 固定为 `pong`。
    pub message: String,
    /// 应用版本号。
    pub app_version: String,
}

/// 连通性检查：前端调用后拿到类型化的返回值，用于验证前后端绑定。
#[tauri::command]
#[specta::specta]
pub fn ping() -> Pong {
    tracing::debug!("ping");
    Pong {
        message: "pong".into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_returns_pong_and_version() {
        let pong = ping();
        assert_eq!(pong.message, "pong");
        assert_eq!(pong.app_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn pong_serializes_in_camel_case() {
        let json = serde_json::to_value(ping()).unwrap();
        assert_eq!(json["message"], "pong");
        assert!(json.get("appVersion").is_some());
    }
}
