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
    db.get_history_grouped(page, page_size, search.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn get_group_history_items(
    app: AppHandle,
    group_id: u64,
) -> Result<Vec<HistoryItem>, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_group_history_items(group_id)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_group(app: AppHandle, group_id: u64) -> Result<(), AppError> {
    let db = app.state::<crate::DbState>();
    db.delete_group_history(group_id)
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

    let file_exists = if let Some(ref item) = history_item {
        match tokio::fs::metadata(&item.file_path).await {
            Ok(meta) => match item.file_size {
                Some(expected) => meta.len() == expected,
                None => true,
            },
            Err(_) => false,
        }
    } else {
        false
    };

    Ok(DuplicateCheckResult {
        in_history: history_item.is_some(),
        in_queue,
        history_item,
        file_exists,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_item(app: AppHandle, id: u64) -> Result<(), AppError> {
    let db = app.state::<crate::DbState>();
    db.delete_history(id)
}
