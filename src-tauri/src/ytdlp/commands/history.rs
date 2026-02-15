use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use tauri::AppHandle;
use tauri::Manager;

#[tauri::command]
#[specta::specta]
pub async fn get_download_history(
    app: AppHandle,
    page: u32,
    page_size: u32,
    search: Option<String>,
) -> Result<HistoryResult, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_history(page, page_size, search.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn check_duplicate(
    app: AppHandle,
    video_id: String,
) -> Result<DuplicateCheckResult, AppError> {
    let db = app.state::<crate::DbState>();
    let history_item = db.check_duplicate(&video_id)?;
    let in_queue = db.check_duplicate_in_queue(&video_id)?;
    Ok(DuplicateCheckResult {
        in_history: history_item.is_some(),
        in_queue,
        history_item,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_item(app: AppHandle, id: u64) -> Result<(), AppError> {
    let db = app.state::<crate::DbState>();
    db.delete_history(id)
}
