use crate::modules::types::AppError;
use crate::ytdlp::types::{LogQueryResult, LogStats};
use tauri::{AppHandle, Manager};

#[tauri::command]
#[specta::specta]
pub async fn get_logs(
    app: AppHandle,
    page: u32,
    page_size: u32,
    level: Option<String>,
    category: Option<String>,
    search: Option<String>,
    since: Option<i64>,
) -> Result<LogQueryResult, AppError> {
    let log_db = app.state::<crate::LogDbState>();
    log_db.query_logs(
        page,
        page_size,
        level.as_deref(),
        category.as_deref(),
        search.as_deref(),
        since,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn get_log_stats(app: AppHandle) -> Result<LogStats, AppError> {
    let log_db = app.state::<crate::LogDbState>();
    log_db.get_log_stats()
}

#[tauri::command]
#[specta::specta]
pub async fn clear_logs(
    app: AppHandle,
    before_timestamp: Option<i64>,
) -> Result<u64, AppError> {
    let log_db = app.state::<crate::LogDbState>();
    log_db.clear_logs(before_timestamp)
}
