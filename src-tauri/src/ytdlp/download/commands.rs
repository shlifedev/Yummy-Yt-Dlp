use super::executor::{execute_download, process_next_pending};
use super::manager::DownloadManager;
use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::settings;
use crate::ytdlp::types::*;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[tauri::command]
#[specta::specta]
pub async fn add_to_queue(app: AppHandle, request: DownloadRequest) -> Result<u64, AppError> {
    // Get settings for download path and filename template
    let settings = settings::get_settings(&app)?;

    // Determine output directory
    let output_dir = request
        .output_dir
        .as_deref()
        .unwrap_or(&settings.download_path);

    // Build output template using OS-native path separators
    let output_template = std::path::Path::new(output_dir)
        .join(&settings.filename_template)
        .to_string_lossy()
        .to_string();

    // Get database from state
    let db_state = app.state::<crate::DbState>();

    // Insert download record into DB with pending status
    let task_id = db_state.insert_download(&request, &output_template)?;

    // Try to acquire a download slot
    let manager = app.state::<Arc<DownloadManager>>();
    if manager.try_acquire() {
        // Immediately start download - ensure release() on DB update failure
        match db_state.update_download_status(task_id, &DownloadStatus::Downloading, None) {
            Ok(()) => {
                let app_clone = app.clone();
                let app_panic_guard = app.clone();
                tokio::spawn(async move {
                    let result = tokio::spawn(async move {
                        execute_download(app_clone, task_id).await;
                    })
                    .await;
                    if let Err(e) = result {
                        logger::error_cat(
                            "download",
                            &format!("[download:{}] task panicked: {:?}", task_id, e),
                        );
                        let manager = app_panic_guard.state::<Arc<DownloadManager>>();
                        manager.release();
                        process_next_pending(app_panic_guard);
                    }
                });
            }
            Err(e) => {
                logger::error_cat(
                    "download",
                    &format!(
                        "[download:{}] failed to update status to downloading: {}",
                        task_id, e
                    ),
                );
                manager.release();
            }
        }
    } else {
        // No slot available - schedule a check for pending items
        // This handles the case where all concurrent downloads finish before
        // batch add_to_queue calls complete, leaving pending items with no trigger.
        let app_clone = app.clone();
        tokio::spawn(async move {
            process_next_pending(app_clone);
        });
    }

    Ok(task_id)
}

#[tauri::command]
#[specta::specta]
pub async fn start_download(app: AppHandle, request: DownloadRequest) -> Result<u64, AppError> {
    // Backward compatibility: delegate to add_to_queue
    add_to_queue(app, request).await
}

// Proper cancel implementation that kills the actual yt-dlp process
#[tauri::command]
#[specta::specta]
pub async fn cancel_download(app: AppHandle, task_id: u64) -> Result<(), AppError> {
    let db_state = app.state::<crate::DbState>();

    // Only cancel if task is still in a cancellable state (pending/downloading).
    // This prevents overwriting a 'completed' status if the download finished
    // between the user clicking cancel and this code executing.
    let was_cancelled = db_state.cancel_if_active(task_id)?;

    if was_cancelled {
        // Send cancel signal to kill the actual yt-dlp process (no-op if not running)
        let manager = app.state::<Arc<DownloadManager>>();
        manager.send_cancel(task_id);
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_all_downloads(app: AppHandle) -> Result<u32, AppError> {
    let db_state = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();

    let ids = db_state.get_cancellable_ids()?;
    let mut cancelled = 0u32;

    for id in ids {
        if db_state.cancel_if_active(id).unwrap_or(false) {
            manager.send_cancel(id);
            cancelled += 1;
        }
    }

    // Sync active_count with actual DB state to correct any drift.
    // Cancel signals are processed asynchronously, so the DB may still show
    // some tasks as 'downloading' briefly. We sync to the current DB truth.
    let actual_active = db_state.get_active_count().unwrap_or(0);
    manager.sync_active_count(actual_active);

    Ok(cancelled)
}

#[tauri::command]
#[specta::specta]
pub async fn pause_download(_app: AppHandle, _task_id: u64) -> Result<(), AppError> {
    Err(AppError::Custom("Not yet implemented".to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn resume_download(_app: AppHandle, _task_id: u64) -> Result<(), AppError> {
    Err(AppError::Custom("Not yet implemented".to_string()))
}
