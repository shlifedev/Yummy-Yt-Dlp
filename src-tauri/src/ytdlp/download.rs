use super::binary;
use super::progress;
use super::settings;
use super::types::*;
use crate::modules::logger;
use crate::modules::types::AppError;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::watch;

const STDERR_BUFFER_LIMIT_BYTES: usize = 64 * 1024;

fn append_limited(buffer: &mut String, line: &str, max_bytes: usize) {
    if !buffer.is_empty() {
        buffer.push('\n');
    }
    buffer.push_str(line);

    if buffer.len() > max_bytes {
        let overflow = buffer.len() - max_bytes;

        let cut_at = if buffer.is_char_boundary(overflow) {
            overflow
        } else {
            buffer
                .char_indices()
                .find(|(idx, _)| *idx > overflow)
                .map(|(idx, _)| idx)
                .unwrap_or(buffer.len())
        };

        buffer.drain(..cut_at);
    }
}

pub struct DownloadManager {
    active_count: AtomicU32,
    max_concurrent: AtomicU32,
    cancel_senders: Mutex<HashMap<u64, watch::Sender<bool>>>,
}

impl DownloadManager {
    pub fn new(max_concurrent: u32) -> Self {
        Self {
            active_count: AtomicU32::new(0),
            max_concurrent: AtomicU32::new(max_concurrent.max(1)),
            cancel_senders: Mutex::new(HashMap::new()),
        }
    }

    pub fn active_count(&self) -> u32 {
        self.active_count.load(Ordering::SeqCst)
    }

    pub fn max_concurrent(&self) -> u32 {
        self.max_concurrent.load(Ordering::SeqCst)
    }

    // 2-2: Prevent set_max_concurrent(0)
    pub fn set_max_concurrent(&self, val: u32) {
        self.max_concurrent.store(val.max(1), Ordering::SeqCst);
    }

    // 1-1: CAS loop to fix TOCTOU race condition
    pub fn try_acquire(&self) -> bool {
        loop {
            let current = self.active_count.load(Ordering::SeqCst);
            if current >= self.max_concurrent.load(Ordering::SeqCst) {
                return false;
            }
            if self
                .active_count
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn release(&self) {
        let _ = self
            .active_count
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
                Some(count.saturating_sub(1))
            });
    }

    /// Synchronize active_count with the actual DB state.
    /// Used after cancel_all to correct any drift between the atomic counter
    /// and the real number of downloading tasks.
    pub fn sync_active_count(&self, count: u32) {
        self.active_count.store(count, Ordering::SeqCst);
    }

    // 1-2: Cancel support methods
    fn register_cancel(&self, task_id: u64) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        senders.insert(task_id, tx);
        rx
    }

    pub fn send_cancel(&self, task_id: u64) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = senders.remove(&task_id) {
            let _ = tx.send(true);
        }
    }

    fn unregister_cancel(&self, task_id: u64) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        senders.remove(&task_id);
    }

    /// 앱 종료 시 모든 활성 다운로드 취소. 동기적으로 cancel signal만 전송.
    pub fn cancel_all(&self) {
        let mut senders = self
            .cancel_senders
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for (_task_id, tx) in senders.drain() {
            let _ = tx.send(true);
        }
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
                        logger::error(&format!(
                            "[download:{}] task panicked: {:?}",
                            task_id, e
                        ));
                        let manager = app_panic_guard.state::<Arc<DownloadManager>>();
                        manager.release();
                        process_next_pending(app_panic_guard);
                    }
                });
            }
            Err(e) => {
                logger::error(&format!(
                    "[download:{}] failed to update status to downloading: {}",
                    task_id, e
                ));
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

/// Public wrapper for execute_download (used by retry_download in commands.rs)
pub async fn execute_download_public(app: AppHandle, task_id: u64) {
    execute_download(app, task_id).await;
}

async fn execute_download(app: AppHandle, task_id: u64) {
    let db_state = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();

    let task = match db_state.get_download(task_id) {
        Ok(Some(t)) => t,
        _ => {
            logger::error(&format!("[download:{}] task not found in DB", task_id));
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    // Guard: if the task was cancelled between being claimed and execution starting, bail out
    if matches!(task.status, DownloadStatus::Cancelled) {
        manager.release();
        process_next_pending(app);
        return;
    }

    let ytdlp_path = match binary::resolve_ytdlp_path().await {
        Ok(p) => p,
        Err(e) => {
            let error_msg = "yt-dlp not found. Please install via Homebrew or click Install.";
            logger::error(&format!("[download:{}] yt-dlp not found: {}", task_id, e));
            let _ = db_state.update_download_status(
                task_id,
                &DownloadStatus::Failed,
                Some(error_msg),
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
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    let settings = match settings::get_settings(&app) {
        Ok(s) => s,
        Err(e) => {
            logger::error(&format!(
                "[download:{}] failed to get settings: {}",
                task_id, e
            ));
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

    // 1-2: Register cancel receiver before spawning process
    let mut cancel_rx = manager.register_cancel(task_id);

    // Build yt-dlp args in a Vec for logging before passing to Command
    let mut args: Vec<String> = Vec::new();
    args.extend(["--format".to_string(), task.format_id.clone()]);
    args.extend(["--output".to_string(), task.output_path.clone()]);
    args.extend([
        "--progress-template".to_string(),
        progress::progress_template(),
    ]);
    args.push("--newline".to_string());
    args.push("--no-playlist".to_string());
    args.push("--no-overwrites".to_string());

    // Force UTF-8 encoding inside yt-dlp (fixes cp949 crash on Korean Windows)
    args.push("--encoding".to_string());
    args.push("UTF-8".to_string());

    // Sanitize filenames for Windows forbidden characters
    #[cfg(target_os = "windows")]
    {
        args.push("--windows-filenames".to_string());
    }

    // Pass ffmpeg location explicitly if available
    if let Some(ffmpeg_path) = binary::resolve_ffmpeg_path().await {
        args.extend(["--ffmpeg-location".to_string(), ffmpeg_path]);
    }

    // Add cookie browser from settings if available
    if let Some(browser) = &settings.cookie_browser {
        args.extend(["--cookies-from-browser".to_string(), browser.clone()]);
    }

    // Add video URL
    args.push(task.video_url.clone());

    // Log the full command before spawning
    logger::info(&format!(
        "[download:{}] spawning: {} {:?}",
        task_id, ytdlp_path, args
    ));

    // Build command with augmented PATH for .app bundles
    let mut cmd = binary::command_with_path(&ytdlp_path);
    cmd.args(&args);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // Spawn process
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let error_msg = format!("Failed to spawn yt-dlp: {}", e);
            logger::error(&format!("[download:{}] {}", task_id, error_msg));
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
            manager.unregister_cancel(task_id);
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            manager.unregister_cancel(task_id);
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            manager.unregister_cancel(task_id);
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    // Clone necessary data for the async task
    let db_state_clone = db_state.inner().clone();
    let app_clone = app.clone();

    // 1-3: Save JoinHandle for stdout reader task
    // Returns the actual output file path parsed from yt-dlp stdout
    let stdout_handle: tokio::task::JoinHandle<Option<String>> = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let mut last_progress_percent: Option<f32> = None;
        let mut last_progress_update = tokio::time::Instant::now() - Duration::from_secs(1);
        let mut actual_file_path: Option<String> = None;

        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) => break, // EOF
                Ok(_) => {}
                Err(_) => continue, // non-fatal read error, keep going
            }
            let line = String::from_utf8_lossy(&buf).trim_end().to_string();
            // Capture actual file path from yt-dlp output lines:
            // "[download] Destination: /path/to/file.mp4"
            // "[Merger] Merging formats into "/path/to/file.mkv""
            // "[ExtractAudio] Destination: /path/to/file.mp3"
            if let Some(path) = line.strip_prefix("[Merger] Merging formats into \"") {
                if let Some(path) = path.strip_suffix('"') {
                    actual_file_path = Some(path.to_string());
                }
            } else if let Some(path) = line
                .strip_prefix("[download] Destination: ")
                .or_else(|| line.strip_prefix("[ExtractAudio] Destination: "))
            {
                actual_file_path = Some(path.trim().to_string());
            }

            if let Some(progress_info) = progress::parse_progress_line(&line) {
                let now = tokio::time::Instant::now();
                let should_update = match last_progress_percent {
                    None => true,
                    Some(prev) => {
                        (progress_info.percent - prev).abs() >= 0.2
                            || now.duration_since(last_progress_update)
                                >= Duration::from_millis(500)
                            || progress_info.percent >= 100.0
                    }
                };

                if !should_update {
                    continue;
                }

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

                last_progress_percent = Some(progress_info.percent);
                last_progress_update = now;
            }
        }

        actual_file_path
    });

    // Collect stderr for error messages (byte-level reader for non-UTF-8 resilience)
    let stderr_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        let mut output = String::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                    append_limited(&mut output, &line, STDERR_BUFFER_LIMIT_BYTES);
                }
                Err(_) => continue,
            }
        }
        output
    });

    // 1-2: Wait for process with cancel support via tokio::select!
    let status = tokio::select! {
        result = child.wait() => {
            match result {
                Ok(s) => s,
                Err(e) => {
                    let error_msg = format!("Failed to wait for process: {}", e);
                    logger::error(&format!("[download:{}] {}", task_id, error_msg));
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
                    let _ = stdout_handle.await;
                    let _ = stderr_handle.await;
                    manager.unregister_cancel(task_id);
                    manager.release();
                    process_next_pending(app);
                    return;
                }
            }
        }
        _ = cancel_rx.changed() => {
            // Cancel signal received - kill the yt-dlp process
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            let _ = db_state.update_download_status(task_id, &DownloadStatus::Cancelled, None);
            let _ = app.emit(
                "download-event",
                GlobalDownloadEvent {
                    task_id,
                    event_type: "cancelled".to_string(),
                    percent: None,
                    speed: None,
                    eta: None,
                    file_path: None,
                    file_size: None,
                    message: Some("다운로드가 취소되었습니다.".to_string()),
                },
            );
            manager.unregister_cancel(task_id);
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    // 1-3, 1-4: Await both stdout and stderr handles before checking result
    let actual_file_path = stdout_handle.await.ok().flatten();
    let stderr_output = stderr_handle.await.unwrap_or_default();

    // Log process exit for debugging
    let exit_code = status.code();
    logger::info(&format!(
        "[download:{}] process exited with code: {:?}",
        task_id, exit_code
    ));
    if !stderr_output.is_empty() {
        logger::warn(&format!(
            "[download:{}] stderr: {}",
            task_id, stderr_output
        ));
    }

    if status.success() {
        // Use the actual file path parsed from yt-dlp stdout, falling back to the template path
        let file_path = actual_file_path.unwrap_or_else(|| task.output_path.clone());
        let file_size = tokio::fs::metadata(&file_path)
            .await
            .ok()
            .map(|m| m.len())
            .unwrap_or(0);

        // Mark as completed and insert history in a single transaction
        let completed_at = chrono::Utc::now().timestamp();
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

        if let Err(e) = db_state.complete_and_record(task_id, completed_at, &history_item) {
            logger::error(&format!(
                "[download:{}] failed to complete_and_record: {}",
                task_id, e
            ));
            // Fallback: at least mark the download as completed
            let _ = db_state.mark_completed(task_id, completed_at);
        }

        logger::info(&format!(
            "[download:{}] completed successfully, file_size={}",
            task_id, file_size
        ));

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
        let error_message = if let Some(code) = status.code() {
            match code {
                1 => {
                    if stderr_output.is_empty() {
                        "다운로드 중 오류가 발생했습니다.".to_string()
                    } else {
                        // Include full stderr for better diagnostics
                        let last_line = stderr_output
                            .lines()
                            .rev()
                            .find(|l| !l.trim().is_empty())
                            .unwrap_or("다운로드 중 오류가 발생했습니다.");
                        format!("{}\n\n[stderr]: {}", last_line, stderr_output)
                    }
                }
                2 => format!(
                    "네트워크 연결 문제입니다. 인터넷 연결을 확인하세요.\n\n[stderr]: {}",
                    stderr_output
                ),
                120 => {
                    // Exit code 120: often a Windows encoding crash (cp949/cp932)
                    let is_encoding_error = stderr_output.contains("cp949")
                        || stderr_output.contains("cp932")
                        || stderr_output.contains("TextIOWrapper")
                        || stderr_output.contains("Errno 22")
                        || stderr_output.contains("UnicodeEncodeError");
                    if is_encoding_error {
                        format!(
                            "인코딩 오류로 다운로드에 실패했습니다.\n\n\
                            Windows 설정 → 시간 및 언어 → 관리 언어 설정 → \
                            시스템 로캘 변경 → 'Beta: 세계 언어 지원을 위해 Unicode UTF-8 사용'을 \
                            활성화한 후 재시작하세요.\n\n[stderr]: {}",
                            stderr_output
                        )
                    } else {
                        format!(
                            "yt-dlp exited with code: 120\n\n[stderr]: {}",
                            stderr_output
                        )
                    }
                }
                _ => format!(
                    "yt-dlp exited with code: {}\n\n[stderr]: {}",
                    code, stderr_output
                ),
            }
        } else {
            format!(
                "다운로드 프로세스가 예기치 않게 종료되었습니다.\n\n[stderr]: {}",
                stderr_output
            )
        };

        logger::error(&format!(
            "[download:{}] failed: {}",
            task_id, error_message
        ));

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
    manager.unregister_cancel(task_id);
    manager.release();
    process_next_pending(app);
}

/// Public wrapper for process_next_pending (used by retry_download in commands.rs)
pub fn process_next_pending_public(app: AppHandle) {
    process_next_pending(app);
}

fn process_next_pending(app: AppHandle) {
    let db_state = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();

    // Try to start pending tasks while slots are available
    while manager.try_acquire() {
        // Use claim_next_pending for atomic dequeue (prevents double-dispatch race condition)
        match db_state.claim_next_pending() {
            Ok(Some(task)) => {
                let app_clone = app.clone();
                let app_panic_guard = app.clone();
                let task_id = task.id;
                tokio::spawn(async move {
                    let result = tokio::spawn(async move {
                        execute_download(app_clone, task_id).await;
                    })
                    .await;
                    if let Err(e) = result {
                        logger::error(&format!(
                            "[download:{}] task panicked: {:?}",
                            task_id, e
                        ));
                        let manager = app_panic_guard.state::<Arc<DownloadManager>>();
                        manager.release();
                        process_next_pending(app_panic_guard);
                    }
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

// 1-2: Proper cancel implementation that kills the actual yt-dlp process
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_limited_keeps_recent_tail() {
        let mut output = String::new();

        append_limited(&mut output, "12345", 8);
        append_limited(&mut output, "6789", 8);

        assert_eq!(output, "345\n6789");
    }
}
