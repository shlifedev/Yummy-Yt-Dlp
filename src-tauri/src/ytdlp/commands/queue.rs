use crate::modules::types::AppError;
use crate::ytdlp::download::DownloadManager;
use crate::ytdlp::types::*;
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;

#[tauri::command]
#[specta::specta]
pub async fn get_download_queue(app: AppHandle) -> Result<Vec<DownloadTaskInfo>, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_download_queue()
}

#[tauri::command]
#[specta::specta]
pub async fn clear_completed(app: AppHandle) -> Result<u32, AppError> {
    let db = app.state::<crate::DbState>();
    db.clear_completed()
}

#[tauri::command]
#[specta::specta]
pub async fn retry_download(app: AppHandle, task_id: u64) -> Result<(), AppError> {
    // Get the original download info from DB
    let db = app.state::<crate::DbState>();
    let _task = db
        .get_download(task_id)?
        .ok_or_else(|| AppError::Custom("Download task not found".to_string()))?;

    // Reset the original task to pending (reuse existing DB row instead of
    // creating a duplicate via add_to_queue, which would leave a zombie pending row)
    db.update_download_status(task_id, &DownloadStatus::Pending, None)?;

    // Try to acquire a slot and start the download immediately if possible
    let manager = app.state::<Arc<DownloadManager>>();
    if manager.try_acquire() {
        db.update_download_status(task_id, &DownloadStatus::Downloading, None)?;
        let app_clone = app.clone();
        let app_panic_guard = app.clone();
        tokio::spawn(async move {
            let result = tokio::spawn(async move {
                crate::ytdlp::download::execute_download_public(app_clone, task_id).await;
            })
            .await;
            if let Err(e) = result {
                eprintln!("Download task panicked: {:?}", e);
                let manager = app_panic_guard.state::<Arc<DownloadManager>>();
                manager.release();
                crate::ytdlp::download::process_next_pending_public(app_panic_guard);
            }
        });
    }
    // Otherwise stays pending, will be picked up by process_next_pending when a slot frees

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_active_downloads(app: AppHandle) -> Result<Vec<DownloadTaskInfo>, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_active_downloads()
}
