use super::binary;
use super::progress;
use super::settings;
use super::types::*;
use crate::modules::types::AppError;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct DownloadManager {
    active_count: AtomicU32,
    max_concurrent: AtomicU32,
}

impl DownloadManager {
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            active_count: AtomicU32::new(0),
            max_concurrent: AtomicU32::new(max_concurrent),
        }
    }

    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    pub fn set_max_concurrent(&self, val: u32) {
        self.max_concurrent.store(val, Ordering::SeqCst);
    }

    pub fn try_acquire(&self) -> bool {
        let current = self.active_count.load(Ordering::SeqCst);
        if current < self.max_concurrent.load(Ordering::SeqCst) {
            self.active_count.fetch_add(1, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn release(&self) {
        self.active_count.fetch_sub(1, Ordering::SeqCst);
    }
}

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

    // Build output template
    let output_template = format!("{}/{}", output_dir, settings.filename_template);

    // Get database from state
    let db_state = app.state::<crate::DbState>();

    // Insert download record into DB with pending status
    let task_id = db_state.insert_download(&request, &output_template)?;

    // Try to acquire a download slot
    let manager = app.state::<Arc<DownloadManager>>();
    if manager.try_acquire() {
        // Immediately start download
        db_state.update_download_status(task_id, &DownloadStatus::Downloading, None)?;
        let app_clone = app.clone();
        tokio::spawn(async move {
            execute_download(app_clone, task_id).await;
        });
    }
    // Otherwise, task stays in pending status until a slot becomes available

    Ok(task_id)
}

async fn execute_download(app: AppHandle, task_id: u64) {
    // Get database from state
    let db_state = app.state::<crate::DbState>();

    // Get task info from DB
    let task = match db_state.get_download(task_id) {
        Ok(Some(t)) => t,
        _ => {
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    // Get app data directory
    let app_data_dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(_) => {
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    // Get binary paths
    let ytdlp_path = match binary::resolve_ytdlp_path(&app_data_dir).await {
        Ok(p) => p,
        Err(_) => {
            let _ = db_state.update_download_status(
                task_id,
                &DownloadStatus::Failed,
                Some("yt-dlp not found. Please install via Homebrew or click Install."),
            );
            let _ = app.emit(
                "download-event",
                GlobalDownloadEvent {
                    task_id,
                    event_type: "error".to_string(),
                    percent: None,
                    speed: None,
                    eta: None,
                    file_path: None,
                    file_size: None,
                    message: Some("yt-dlp not found".to_string()),
                },
            );
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };
    let ffmpeg_path = binary::get_ffmpeg_path(&app_data_dir);

    // Get settings for cookie browser and other options
    let settings = match settings::get_settings(&app) {
        Ok(s) => s,
        Err(_) => {
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    // Send started event
    let _ = app.emit(
        "download-event",
        GlobalDownloadEvent {
            task_id,
            event_type: "started".to_string(),
            percent: None,
            speed: None,
            eta: None,
            file_path: None,
            file_size: None,
            message: None,
        },
    );

    // Build yt-dlp command
    let mut cmd = tokio::process::Command::new(&ytdlp_path);

    cmd.arg("--format").arg(&task.format_id);

    cmd.arg("--output").arg(&task.output_path);

    // Get ffmpeg directory (parent of ffmpeg binary)
    if let Some(ffmpeg_dir) = ffmpeg_path.parent() {
        cmd.arg("--ffmpeg-location").arg(ffmpeg_dir);
    }

    cmd.arg("--progress-template")
        .arg(progress::progress_template());
    cmd.arg("--newline");
    cmd.arg("--no-playlist");

    // Add cookie browser from settings if available
    if let Some(browser) = &settings.cookie_browser {
        cmd.arg("--cookies-from-browser").arg(browser);
    }

    // Add video URL
    cmd.arg(&task.video_url);

    // Disable Python's stdout buffering
    cmd.env("PYTHONUNBUFFERED", "1");

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Spawn process
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let error_msg = format!("Failed to spawn yt-dlp: {}", e);
            let _ =
                db_state.update_download_status(task_id, &DownloadStatus::Failed, Some(&error_msg));
            let _ = app.emit(
                "download-event",
                GlobalDownloadEvent {
                    task_id,
                    event_type: "error".to_string(),
                    percent: None,
                    speed: None,
                    eta: None,
                    file_path: None,
                    file_size: None,
                    message: Some(error_msg),
                },
            );
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    // Clone necessary data for the async task
    let db_state_clone = db_state.inner().clone();
    let app_clone = app.clone();

    // Spawn task to read progress from stdout
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(progress_info) = progress::parse_progress_line(&line) {
                let speed = progress_info.speed.as_deref().unwrap_or("...").to_string();
                let eta = progress_info.eta.as_deref().unwrap_or("...").to_string();

                // Send global progress event
                let _ = app_clone.emit(
                    "download-event",
                    GlobalDownloadEvent {
                        task_id,
                        event_type: "progress".to_string(),
                        percent: Some(progress_info.percent),
                        speed: Some(speed.clone()),
                        eta: Some(eta.clone()),
                        file_path: None,
                        file_size: None,
                        message: None,
                    },
                );

                // Update DB progress
                let _ = db_state_clone.update_download_progress(
                    task_id,
                    progress_info.percent,
                    Some(&speed),
                    Some(&eta),
                );
            }
        }
    });

    // Collect stderr for error messages
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut output = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&line);
        }
        output
    });

    // Wait for process to complete
    let status = match child.wait().await {
        Ok(s) => s,
        Err(e) => {
            let error_msg = format!("Failed to wait for process: {}", e);
            let _ =
                db_state.update_download_status(task_id, &DownloadStatus::Failed, Some(&error_msg));
            let _ = app.emit(
                "download-event",
                GlobalDownloadEvent {
                    task_id,
                    event_type: "error".to_string(),
                    percent: None,
                    speed: None,
                    eta: None,
                    file_path: None,
                    file_size: None,
                    message: Some(error_msg),
                },
            );
            let manager = app.state::<Arc<DownloadManager>>();
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    if status.success() {
        // Download completed successfully
        let file_path = task.output_path.clone();
        let file_size = std::fs::metadata(&file_path)
            .ok()
            .map(|m| m.len())
            .unwrap_or(0);

        // Mark as completed in DB
        let completed_at = chrono::Utc::now().timestamp();
        let _ = db_state.mark_completed(task_id, completed_at);

        // Insert into history
        let history_item = HistoryItem {
            id: 0,
            video_url: task.video_url.clone(),
            video_id: task.video_id.clone(),
            title: task.title.clone(),
            quality_label: task.quality_label.clone(),
            format: task.format_id.clone(),
            file_path: file_path.clone(),
            file_size: Some(file_size),
            downloaded_at: completed_at,
        };

        let _ = db_state.insert_history(&history_item);

        // Send completion event
        let _ = app.emit(
            "download-event",
            GlobalDownloadEvent {
                task_id,
                event_type: "completed".to_string(),
                percent: Some(100.0),
                speed: None,
                eta: None,
                file_path: Some(file_path),
                file_size: Some(file_size),
                message: None,
            },
        );
    } else {
        // Download failed
        let stderr_output = stderr_handle.await.unwrap_or_default();

        let error_message = if let Some(code) = status.code() {
            match code {
                1 => {
                    if stderr_output.is_empty() {
                        "다운로드 중 오류가 발생했습니다.".to_string()
                    } else {
                        stderr_output
                            .lines()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("다운로드 중 오류가 발생했습니다.")
                            .to_string()
                    }
                }
                2 => "네트워크 연결 문제입니다. 인터넷 연결을 확인하세요.".to_string(),
                _ => format!("yt-dlp exited with code: {}", code),
            }
        } else {
            "다운로드 프로세스가 예기치 않게 종료되었습니다.".to_string()
        };

        let _ =
            db_state.update_download_status(task_id, &DownloadStatus::Failed, Some(&error_message));

        let _ = app.emit(
            "download-event",
            GlobalDownloadEvent {
                task_id,
                event_type: "error".to_string(),
                percent: None,
                speed: None,
                eta: None,
                file_path: None,
                file_size: None,
                message: Some(error_message),
            },
        );
    }

    // Release the download slot and process next pending
    let manager = app.state::<Arc<DownloadManager>>();
    manager.release();
    process_next_pending(app);
}

fn process_next_pending(app: AppHandle) {
    let db_state = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();

    // Try to start pending tasks while slots are available
    while manager.try_acquire() {
        match db_state.get_next_pending() {
            Ok(Some(task)) => {
                let _ =
                    db_state.update_download_status(task.id, &DownloadStatus::Downloading, None);
                let app_clone = app.clone();
                let task_id = task.id;
                tokio::spawn(async move {
                    execute_download(app_clone, task_id).await;
                });
            }
            _ => {
                // No more pending tasks, release the slot
                manager.release();
                break;
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn start_download(
    app: AppHandle,
    request: DownloadRequest,
    on_event: Channel<DownloadEvent>,
) -> Result<u64, AppError> {
    // Backward compatibility: delegate to add_to_queue
    // The on_event channel is no longer used as we emit global events
    let _ = on_event; // Suppress unused warning
    add_to_queue(app, request).await
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_download(app: AppHandle, task_id: u64) -> Result<(), AppError> {
    let db_state = app.state::<crate::DbState>();

    // Update status to Cancelled in DB
    db_state.update_download_status(task_id, &DownloadStatus::Cancelled, None)?;

    // TODO: Implement process killing in future enhancement
    // For now, just update the status

    Ok(())
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
