//! 暴露给前端的 Tauri 命令。签名由 tauri-specta 导出到 `ui/src/bindings.ts`。
//!
//! 业务逻辑都在 [`AppState`] 里，这里只做参数转换；失败时返回 [`ErrorDto`]。

use std::path::PathBuf;

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::dto::{
    AccountDto, AccountKey, AccountStatsDto, AppInfo, BackupDto, CmdResult, DirKind, DiscoveredUrl,
    ErrorDto, ExportFormat, ExportResult, FetchModeDto, FilePreviewDto, GameDto, MetaInfo,
    MetaUpdateResult, OverviewEntry, RecordPage, RecordQueryDto, Settings, UrlSource,
};
use crate::state::AppState;

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

fn err(e: gachanote_core::AppError) -> ErrorDto {
    if !matches!(e, gachanote_core::AppError::Cancelled) {
        tracing::warn!(code = e.code(), "命令失败");
    }
    e.into()
}

/// 游戏列表、卡池类型与支持的导入方式。
#[tauri::command]
#[specta::specta]
pub fn list_games(state: State<'_, AppState>) -> Vec<GameDto> {
    state.list_games()
}

/// 全部账号。
#[tauri::command]
#[specta::specta]
pub fn list_accounts(state: State<'_, AppState>) -> CmdResult<Vec<AccountDto>> {
    state.list_accounts().map_err(err)
}

/// 修改备注名；`nickname` 为空表示清除。
#[tauri::command]
#[specta::specta]
pub fn rename_account(
    state: State<'_, AppState>,
    account_id: String,
    nickname: Option<String>,
) -> CmdResult<()> {
    state
        .rename_account(&account_id, nickname.as_deref())
        .map_err(err)
}

/// 删除账号及其全部记录（删除前自动备份）。
#[tauri::command]
#[specta::specta]
pub fn delete_account(state: State<'_, AppState>, account_id: String) -> CmdResult<()> {
    state.delete_account(&account_id).map_err(err)
}

/// 一个账号的统计。
#[tauri::command]
#[specta::specta]
pub fn get_stats(state: State<'_, AppState>, account_id: String) -> CmdResult<AccountStatsDto> {
    state.get_stats(&account_id).map_err(err)
}

/// 跨游戏总览。
#[tauri::command]
#[specta::specta]
pub fn get_overview(state: State<'_, AppState>) -> CmdResult<Vec<OverviewEntry>> {
    state.get_overview().map_err(err)
}

/// 分页查询记录。
#[tauri::command]
#[specta::specta]
pub fn query_records(state: State<'_, AppState>, query: RecordQueryDto) -> CmdResult<RecordPage> {
    state.query_records(query).map_err(err)
}

/// 识别文件格式并预览（不写入）。
#[tauri::command]
#[specta::specta]
pub fn sniff_file(state: State<'_, AppState>, path: String) -> CmdResult<FilePreviewDto> {
    state.preview_file(&PathBuf::from(path)).map_err(err)
}

/// 后台导入文件，返回任务 ID；结果通过 `task://*` 事件推送。
#[tauri::command]
#[specta::specta]
pub fn import_file(state: State<'_, AppState>, path: String, only: Option<Vec<AccountKey>>) -> u64 {
    state.import_file(PathBuf::from(path), only)
}

/// 在游戏缓存或日志里查找抽卡链接。
#[tauri::command]
#[specta::specta]
pub async fn discover_url(state: State<'_, AppState>, game: String) -> CmdResult<DiscoveredUrl> {
    state.discover_url(&game).await.map_err(err)
}

/// 后台按链接拉取，返回任务 ID。
#[tauri::command]
#[specta::specta]
pub fn fetch_by_url(
    state: State<'_, AppState>,
    game: String,
    source: UrlSource,
    mode: FetchModeDto,
) -> CmdResult<u64> {
    state.fetch_by_url(&game, source, mode).map_err(err)
}

/// 取消导入或拉取。
#[tauri::command]
#[specta::specta]
pub fn cancel_task(state: State<'_, AppState>, task_id: u64) -> bool {
    state.cancel_task(task_id)
}

/// 导出账号到文件。
#[tauri::command]
#[specta::specta]
pub fn export_accounts(
    state: State<'_, AppState>,
    account_ids: Vec<String>,
    format: ExportFormat,
    path: String,
) -> CmdResult<ExportResult> {
    state
        .export(&account_ids, format, &PathBuf::from(path))
        .map_err(err)
}

/// 读取设置。
#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    state.get_settings().map_err(err)
}

/// 保存设置。
#[tauri::command]
#[specta::specta]
pub fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    state.set_settings(&settings).map_err(err)
}

/// 应用与目录信息。
#[tauri::command]
#[specta::specta]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    state.app_info()
}

/// 更改数据目录（复制数据库与备份），重启后生效。
#[tauri::command]
#[specta::specta]
pub fn set_data_dir(state: State<'_, AppState>, dir: String) -> CmdResult<String> {
    state.set_data_dir(&PathBuf::from(dir)).map_err(err)
}

/// 用系统文件管理器打开数据、备份或日志目录。
#[tauri::command]
#[specta::specta]
pub fn open_dir(state: State<'_, AppState>, kind: DirKind) -> CmdResult<()> {
    let dir = state.dir(kind);
    std::fs::create_dir_all(&dir).map_err(|e| err(e.into()))?;
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| err(e.into()))
}

/// 重启应用（更改数据目录或代理后）。
#[tauri::command]
#[specta::specta]
pub fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

/// 全部备份。
#[tauri::command]
#[specta::specta]
pub fn list_backups(state: State<'_, AppState>) -> CmdResult<Vec<BackupDto>> {
    state.list_backups().map_err(err)
}

/// 立即备份。
#[tauri::command]
#[specta::specta]
pub fn create_backup(state: State<'_, AppState>) -> CmdResult<BackupDto> {
    state.create_backup().map_err(err)
}

/// 从备份恢复（恢复前会先备份当前数据）。
#[tauri::command]
#[specta::specta]
pub fn restore_backup(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    state.restore_backup(&name).map_err(err)
}

/// 当前卡池数据包。
#[tauri::command]
#[specta::specta]
pub fn meta_info(state: State<'_, AppState>) -> MetaInfo {
    state.meta_info()
}

/// 检查并下载卡池数据包。
#[tauri::command]
#[specta::specta]
pub async fn update_metadata(state: State<'_, AppState>) -> CmdResult<MetaUpdateResult> {
    state.update_metadata().await.map_err(err)
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
