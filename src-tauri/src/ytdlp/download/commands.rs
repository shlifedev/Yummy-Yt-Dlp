use super::executor::{execute_download, finalize_panicked_download, process_next_pending};
use super::manager::DownloadManager;
use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use crate::ytdlp::{security, settings};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[tauri::command]
#[specta::specta]
pub async fn add_to_queue(app: AppHandle, request: DownloadRequest) -> Result<u64, AppError> {
    // Validate URL
    security::sanitize_url(&request.video_url)?;

    // Get settings for download path and filename template
    let settings = settings::get_settings(&app)?;

    // Determine output directory and validate path
    let output_dir = request
        .output_dir
        .as_deref()
        .unwrap_or(&settings.download_path);
    security::sanitize_output_path(output_dir)?;

    // Re-validate the filename template here, not just in update_settings: settings.json can be
    // edited on disk to slip a traversal/dangerous template past the UI. Validate before joining.
    security::sanitize_filename_template(&settings.filename_template)?;

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
        // Atomically claim the freshly inserted row (pending -> downloading). In the window
        // since the insert, a concurrent process_next_pending — kicked by any finishing
        // download — can claim this same row, and a cancel can flip it to 'cancelled'; an
        // unconditional status write here would double-dispatch the task or resurrect it.
        match db_state.claim_specific_pending(task_id) {
            Ok(true) => {
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
                        finalize_panicked_download(
                            app_panic_guard,
                            task_id,
                            format!("internal panic: {:?}", e),
                        )
                        .await;
                    }
                });
            }
            Ok(false) => {
                // Another scheduler already owns the row (or it was cancelled): give the slot
                // back and keep the queue draining in case other pending rows are waiting.
                manager.release();
                let app_clone = app.clone();
                tokio::spawn(async move {
                    process_next_pending(app_clone);
                });
            }
            Err(e) => {
                logger::error_cat(
                    "download",
                    &format!("[download:{}] failed to claim queued row: {}", task_id, e),
                );
                manager.release();
                // Re-kick the scheduler so the still-pending row is re-attempted through the
                // atomic claim path once the DB recovers, instead of sitting 'pending' with no
                // dispatch trigger until unrelated queue activity.
                let app_clone = app.clone();
                tokio::spawn(async move {
                    process_next_pending(app_clone);
                });
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

    Ok(cancelled)
}

/// Add multiple downloads as one batch. When `group_title` is set and there are
/// 2+ requests, they are wrapped in a download group; otherwise each is inserted
/// standalone. All rows go in as `pending` and the scheduler is kicked once.
#[tauri::command]
#[specta::specta]
pub async fn add_to_queue_batch(
    app: AppHandle,
    requests: Vec<DownloadRequest>,
    group_title: Option<String>,
    group_kind: Option<String>,
) -> Result<BatchEnqueueResult, AppError> {
    let settings = settings::get_settings(&app)?;

    // Defense-in-depth against a hand-edited settings.json (see add_to_queue).
    security::sanitize_filename_template(&settings.filename_template)?;

    // Validate each request and build its output template (same as add_to_queue).
    // Invalid entries are skipped (and logged) instead of `?`-failing the whole batch,
    // so one bad URL can't poison the other items.
    let mut items = Vec::with_capacity(requests.len());
    for req in &requests {
        if let Err(e) = security::sanitize_url(&req.video_url) {
            logger::warn_cat(
                "download",
                &format!("Skipping invalid batch entry '{}': {}", req.video_url, e),
            );
            continue;
        }
        let output_dir = req.output_dir.as_deref().unwrap_or(&settings.download_path);
        if let Err(e) = security::sanitize_output_path(output_dir) {
            logger::warn_cat(
                "download",
                &format!(
                    "Skipping batch entry '{}': invalid output path: {}",
                    req.video_url, e
                ),
            );
            continue;
        }
        let output_template = std::path::Path::new(output_dir)
            .join(&settings.filename_template)
            .to_string_lossy()
            .to_string();
        items.push((req.clone(), output_template));
    }

    // Never create an empty group: if every entry was invalid, fail loudly instead.
    if items.is_empty() {
        return Err(AppError::InvalidUrl("No valid URLs in batch".to_string()));
    }

    let db_state = app.state::<crate::DbState>();
    let kind = group_kind.unwrap_or_else(|| "playlist".to_string());
    let (group_id, task_ids) =
        db_state.insert_group_with_downloads(group_title.as_deref(), &kind, &items)?;

    // Everything is pending now. Kick the scheduler once — process_next_pending
    // claims slots atomically up to max_concurrent. Never try_acquire per item here.
    let app_clone = app.clone();
    tokio::spawn(async move {
        process_next_pending(app_clone);
    });

    Ok(BatchEnqueueResult { group_id, task_ids })
}

/// Cancel every still-active (pending/downloading) item in a group. Same pattern
/// as cancel_all_downloads, scoped to the group.
#[tauri::command]
#[specta::specta]
pub async fn cancel_group(app: AppHandle, group_id: u64) -> Result<u32, AppError> {
    let db_state = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();

    let ids = db_state.get_cancellable_ids_in_group(group_id)?;
    let mut cancelled = 0u32;

    for id in ids {
        if db_state.cancel_if_active(id).unwrap_or(false) {
            manager.send_cancel(id);
            cancelled += 1;
        }
    }

    Ok(cancelled)
}

#[tauri::command]
#[specta::specta]
pub async fn pause_download(_app: AppHandle, _task_id: u64) -> Result<(), AppError> {
    Err(AppError::NotImplemented("pause_download".to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn resume_download(_app: AppHandle, _task_id: u64) -> Result<(), AppError> {
    Err(AppError::NotImplemented("resume_download".to_string()))
}
