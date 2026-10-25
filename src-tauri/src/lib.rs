//! GachaNote 桌面端外壳：注册 Tauri 命令、初始化日志，是各 crate 唯一的组装点。

mod commands;
mod logging;

use tauri::Manager;
use tauri_specta::{collect_commands, Builder};

/// 前后端类型绑定的输出位置，由 tauri-specta 生成，禁止手改。
#[cfg(any(debug_assertions, test))]
const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/bindings.ts");

/// 汇总所有暴露给前端的命令。新增命令后需重新生成 `ui/src/bindings.ts`
/// （`cargo test -p gachanote-app export_bindings`，或直接运行 `pnpm tauri dev`）。
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![commands::ping])
}

/// 把命令签名导出为 TypeScript。
#[cfg(any(debug_assertions, test))]
fn export_bindings(builder: &Builder<tauri::Wry>) {
    builder
        .export(specta_typescript::Typescript::default(), BINDINGS_PATH)
        .expect("导出 bindings.ts 失败");
}

/// 启动应用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    // 开发时每次启动都刷新绑定，省去手动运行导出测试
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            let log_dir = app.path().data_dir()?.join("GachaNote").join("logs");
            let guard = logging::init(&log_dir)?;
            app.manage(guard);
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "GachaNote 启动");

            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI 用它重新生成 `ui/src/bindings.ts`，并检查与已提交的版本一致。
    #[test]
    fn export_bindings() {
        super::export_bindings(&specta_builder());
    }
}
