use crate::modules::types::AppError;
use crate::ytdlp::download::DownloadManager;
use crate::ytdlp::types::*;
use std::sync::Arc;
use std::time::Duration;
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
pub async fn get_active_queue(app: AppHandle) -> Result<Vec<DownloadTaskInfo>, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_active_queue()
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
    let db = app.state::<crate::DbState>();
    // Ensure the task exists (reuse the existing row rather than creating a duplicate).
    db.get_download(task_id)?
        .ok_or_else(|| AppError::Custom("Download task not found".to_string()))?;

    let manager = app.state::<Arc<DownloadManager>>();

    // An executor for this task is still in flight — typically winding down after a cancel
    // (the kill + stream draining run for seconds after the row already reads 'cancelled').
    // Spawning a second executor now would race the stale one, so re-queue as 'pending'
    // instead: the old attempt's terminal path unregisters its cancel sender, releases its
    // slot, and kicks process_next_pending, which then claims this row cleanly.
    if manager.is_executing(task_id) {
        if db.queue_for_retry(task_id)? {
            if !manager.is_executing(task_id) {
                // The old executor finished between the check above and the re-queue, so its
                // scheduler kick may have run too early to see this row — kick again.
                let app_clone = app.clone();
                tokio::spawn(async move {
                    crate::ytdlp::download::process_next_pending_public(app_clone);
                });
            }
        } else {
            crate::modules::logger::warn_cat(
                "download",
                &format!(
                    "[download:{}] retry ignored: task not in a retryable state",
                    task_id
                ),
            );
        }
        return Ok(());
    }

    if manager.try_acquire() {
        // Atomically flip this task to 'downloading' only if it is still retryable. This both
        // pairs the acquired slot with a release on every failure path and prevents a concurrent
        // process_next_pending from claiming the same task (double-dispatch).
        match db.claim_for_retry(task_id) {
            Ok(true) => {
                let app_clone = app.clone();
                let app_panic_guard = app.clone();
                tokio::spawn(async move {
                    let result = tokio::spawn(async move {
                        crate::ytdlp::download::execute_download_public(app_clone, task_id).await;
                    })
                    .await;
                    if let Err(e) = result {
                        crate::modules::logger::error_cat(
                            "download",
                            &format!("[download:{}] task panicked: {:?}", task_id, e),
                        );
                        crate::ytdlp::download::finalize_panicked_download(
                            app_panic_guard,
                            task_id,
                            format!("internal panic: {:?}", e),
                        )
                        .await;
                    }
                });
            }
            // Not in a retryable state (e.g. already running/completed): give the slot back.
            Ok(false) => {
                crate::modules::logger::warn_cat(
                    "download",
                    &format!(
                        "[download:{}] retry ignored: task not in a retryable state",
                        task_id
                    ),
                );
                manager.release();
            }
            Err(e) => {
                manager.release();
                return Err(e);
            }
        }
    } else {
        // No slot free: reset to pending so process_next_pending picks it up atomically when a
        // slot frees. The extra trigger covers a slot freeing up between try_acquire and now.
        if db.queue_for_retry(task_id)? {
            let app_clone = app.clone();
            tokio::spawn(async move {
                crate::ytdlp::download::process_next_pending_public(app_clone);
            });
        } else {
            crate::modules::logger::warn_cat(
                "download",
                &format!(
                    "[download:{}] retry ignored: task not in a retryable state",
                    task_id
                ),
            );
        }
    }

    Ok(())
}

/// Wipe the entire queue and download history in one shot. Cancels anything in flight first so
/// yt-dlp processes stop before their rows disappear, then clears both tables.
#[tauri::command]
#[specta::specta]
pub async fn clear_all_queue_and_history(app: AppHandle) -> Result<(), AppError> {
    let db = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();

    let ids = db.get_cancellable_ids()?;
    for id in ids {
        db.cancel_if_active(id).ok();
        manager.send_cancel(id);
    }

    let _ = manager.wait_until_idle(Duration::from_secs(5)).await;
    db.clear_all_data()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn get_active_downloads(app: AppHandle) -> Result<Vec<DownloadTaskInfo>, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_active_downloads()
}

#[tauri::command]
#[specta::specta]
pub async fn get_queue_summary(app: AppHandle) -> Result<QueueSummary, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_queue_summary(5)
}
