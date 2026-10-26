//! GachaNote 桌面端外壳：注册 Tauri 命令与事件、初始化日志与应用状态，是各 crate 唯一的组装点。

mod commands;
mod dto;
mod logging;
mod state;

use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_specta::{collect_commands, collect_events, Builder, ErrorHandlingMode, Event};

use state::{AppEvent, AppState, EventSink, Paths};

/// 前后端类型绑定的输出位置，由 tauri-specta 生成，禁止手改。
#[cfg(any(debug_assertions, test))]
const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/src/bindings.ts");

/// 汇总所有暴露给前端的命令与事件。新增后需重新生成 `ui/src/bindings.ts`
/// （`cargo test -p gachanote-app export_bindings`，或直接运行 `pnpm tauri dev`）。
fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        // 失败的命令直接抛出 `ErrorDto`，由前端 `api/` 统一转成提示
        .error_handling(ErrorHandlingMode::Throw)
        // 时间戳（毫秒）与任务 ID 都远小于 2^53，按 number 传输是安全的
        .dangerously_cast_bigints_to_number()
        // DTO 都同时实现 Serialize 与 Deserialize，形状一致，不必分两套类型
        .disable_serde_phases()
        .commands(collect_commands![
            commands::ping,
            commands::list_games,
            commands::list_accounts,
            commands::rename_account,
            commands::delete_account,
            commands::get_stats,
            commands::get_overview,
            commands::query_records,
            commands::sniff_file,
            commands::import_file,
            commands::discover_url,
            commands::fetch_by_url,
            commands::cancel_task,
            commands::export_accounts,
            commands::get_settings,
            commands::set_settings,
            commands::app_info,
            commands::set_data_dir,
            commands::open_dir,
            commands::restart_app,
            commands::list_backups,
            commands::create_backup,
            commands::restore_backup,
            commands::meta_info,
            commands::update_metadata,
        ])
        .events(collect_events![
            dto::TaskProgress,
            dto::TaskDone,
            dto::TaskFailed,
            dto::DataChanged,
            dto::MetaUpdated,
        ])
}

/// 把命令签名导出为 TypeScript。
#[cfg(any(debug_assertions, test))]
fn export_bindings(builder: &Builder<tauri::Wry>) {
    builder
        .export(specta_typescript::Typescript::default(), BINDINGS_PATH)
        .expect("导出 bindings.ts 失败");
}

/// 把后端事件转成 tauri-specta 事件发给前端。
struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: AppEvent) {
        let result = match event {
            AppEvent::Progress(e) => e.emit(&self.0),
            AppEvent::Done(e) => e.emit(&self.0),
            AppEvent::Failed(e) => e.emit(&self.0),
            AppEvent::DataChanged(account_ids) => dto::DataChanged { account_ids }.emit(&self.0),
            AppEvent::MetaUpdated(version) => dto::MetaUpdated { version }.emit(&self.0),
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, "发送事件失败");
        }
    }
}

/// 启动应用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();

    // 开发时每次启动都刷新绑定，省去手动运行导出测试
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            let base = app.path().data_dir()?.join("GachaNote");
            let paths = Paths::resolve(base);
            let guard = logging::init(&paths.logs())?;
            app.manage(guard);
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "GachaNote 启动");

            builder.mount_events(app);
            let sink = Arc::new(TauriSink(app.handle().clone()));
            let state = AppState::open(paths, sink)?;
            let auto_update = state
                .get_settings()
                .map(|s| s.meta_auto_update)
                .unwrap_or(true);
            app.manage(state);

            // 启动时在后台检查卡池数据包更新（F-08），失败只记日志，继续用当前的包
            if auto_update {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    if let Err(e) = state.update_metadata().await {
                        tracing::warn!(error = %e, "检查卡池数据包更新失败");
                    }
                });
            }
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
