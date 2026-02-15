use super::manager::DownloadManager;
use crate::modules::logger;
use crate::ytdlp::types::*;
use crate::ytdlp::{binary, progress, security, settings};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};

const STDERR_BUFFER_LIMIT_BYTES: usize = 64 * 1024;
const KILL_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum duration for a single download (6 hours)
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(6 * 60 * 60);

/// Kill a child process and all its descendants (e.g., ffmpeg spawned by yt-dlp).
/// On Windows, uses `taskkill /F /T /PID` to kill the entire process tree.
/// On Unix, sends SIGKILL to the process directly. Falls back to tokio child.kill().
/// Includes a timeout to prevent hanging if the process doesn't respond.
async fn kill_process_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            // taskkill /F (force) /T (tree - kill child processes) /PID
            let _ = tokio::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output()
                .await;
        }
        #[cfg(unix)]
        {
            // Send SIGKILL to the process directly.
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
    }

    // Fallback: standard kill via tokio
    let _ = child.kill().await;

    // Wait for the process to exit with a timeout to prevent indefinite hanging
    let _ = tokio::time::timeout(KILL_TIMEOUT, child.wait()).await;
}

/// Helper: emit an error download event to the frontend.
/// Sanitizes the error message to remove sensitive system paths before sending to UI.
fn emit_download_error(app: &AppHandle, task_id: u64, message: String) {
    let sanitized = security::sanitize_error_message(&message);
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
            message: Some(sanitized),
        },
    );
}

/// Helper: handle a fatal download error by logging, updating DB, emitting event,
/// and releasing the slot.
fn handle_download_failure(
    app: &AppHandle,
    task_id: u64,
    error_msg: &str,
    db: &crate::DbState,
    manager: &Arc<DownloadManager>,
) {
    logger::error_cat("download", &format!("[download:{}] {}", task_id, error_msg));
    let _ = db.update_download_status(task_id, &DownloadStatus::Failed, Some(error_msg));
    emit_download_error(app, task_id, error_msg.to_string());
    manager.unregister_cancel(task_id);
    manager.release();
    process_next_pending(app.clone());
}

pub(super) fn append_limited(buffer: &mut String, line: &str, max_bytes: usize) {
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

/// Public wrapper for execute_download (used by retry_download in commands.rs)
pub async fn execute_download_public(app: AppHandle, task_id: u64) {
    execute_download(app, task_id).await;
}

pub(super) async fn execute_download(app: AppHandle, task_id: u64) {
    let db_state = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();

    let task = match db_state.get_download(task_id) {
        Ok(Some(t)) => t,
        _ => {
            logger::error_cat(
                "download",
                &format!("[download:{}] task not found in DB", task_id),
            );
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

    let ytdlp_path = match binary::resolve_ytdlp_path_with_app(&app).await {
        Ok(p) => p,
        Err(_e) => {
            let error_msg = "yt-dlp not found. Please install via Homebrew or click Install.";
            logger::error_cat(
                "download",
                &format!("[download:{}] yt-dlp not found: {}", task_id, _e),
            );
            let _ =
                db_state.update_download_status(task_id, &DownloadStatus::Failed, Some(error_msg));
            emit_download_error(&app, task_id, "yt-dlp not found".to_string());
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    let settings = match settings::get_settings(&app) {
        Ok(s) => s,
        Err(e) => {
            logger::error_cat(
                "download",
                &format!("[download:{}] failed to get settings: {}", task_id, e),
            );
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

    // Register cancel receiver before spawning process
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
    if let Some(ffmpeg_path) = binary::resolve_ffmpeg_path_with_app(&app).await {
        args.extend(["--ffmpeg-location".to_string(), ffmpeg_path]);
    }

    // Add cookie browser from settings if available (validated)
    if let Some(browser) = &settings.cookie_browser {
        if security::sanitize_cookie_browser(browser).is_ok() {
            args.extend(["--cookies-from-browser".to_string(), browser.clone()]);
        } else {
            logger::warn_cat(
                "download",
                &format!(
                    "[download:{}] skipping invalid cookie_browser: {}",
                    task_id, browser
                ),
            );
        }
    }

    // Add video URL
    args.push(task.video_url.clone());

    // Log the full command before spawning
    logger::info_cat(
        "download",
        &format!("[download:{}] spawning: {} {:?}", task_id, ytdlp_path, args),
    );

    // Build command with augmented PATH including app bin dir
    let mut cmd = binary::command_with_path_app(&ytdlp_path, &app);
    cmd.args(&args);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // Spawn process
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let error_msg = format!("Failed to spawn yt-dlp: {}", e);
            handle_download_failure(&app, task_id, &error_msg, &db_state, &manager);
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

    // Save JoinHandle for stdout reader task
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

    // Wait for process with cancel support and overall timeout via tokio::select!
    let status = tokio::select! {
        result = child.wait() => {
            match result {
                Ok(s) => s,
                Err(e) => {
                    let error_msg = format!("Failed to wait for process: {}", e);
                    let _ = stdout_handle.await;
                    let _ = stderr_handle.await;
                    handle_download_failure(&app, task_id, &error_msg, &db_state, &manager);
                    return;
                }
            }
        }
        _ = tokio::time::sleep(DOWNLOAD_TIMEOUT) => {
            // Download timeout reached - kill the process
            logger::error_cat(
                "download",
                &format!("[download:{}] timed out after {:?}", task_id, DOWNLOAD_TIMEOUT),
            );
            kill_process_tree(&mut child).await;
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            let error_msg = "다운로드 시간이 초과되었습니다 (최대 6시간).";
            let _ = db_state.update_download_status(task_id, &DownloadStatus::Failed, Some(error_msg));
            emit_download_error(&app, task_id, error_msg.to_string());
            manager.unregister_cancel(task_id);
            manager.release();
            process_next_pending(app);
            return;
        }
        _ = cancel_rx.changed() => {
            // Cancel signal received - kill the yt-dlp process and its children (e.g., ffmpeg)
            kill_process_tree(&mut child).await;
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

    // Await both stdout and stderr handles before checking result
    let actual_file_path = stdout_handle.await.ok().flatten();
    let stderr_output = stderr_handle.await.unwrap_or_default();

    // Log process exit for debugging
    let exit_code = status.code();
    logger::info_cat(
        "download",
        &format!(
            "[download:{}] process exited with code: {:?}",
            task_id, exit_code
        ),
    );
    if !stderr_output.is_empty() {
        logger::warn_cat(
            "download",
            &format!("[download:{}] stderr: {}", task_id, stderr_output),
        );
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
            logger::error_cat(
                "download",
                &format!(
                    "[download:{}] failed to complete_and_record: {}",
                    task_id, e
                ),
            );
            // Fallback: at least mark the download as completed
            let _ = db_state.mark_completed(task_id, completed_at);
        }

        logger::info_cat(
            "download",
            &format!(
                "[download:{}] completed successfully, file_size={}",
                task_id, file_size
            ),
        );

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
                    if stderr_output.contains("Could not copy") && stderr_output.contains("cookie")
                    {
                        "브라우저 쿠키에 접근할 수 없습니다. 브라우저를 완전히 종료하거나, Firefox 쿠키를 사용하세요.".to_string()
                    } else if stderr_output.is_empty() {
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

        // Log full error internally, sanitize for frontend
        logger::error_cat(
            "download",
            &format!("[download:{}] failed: {}", task_id, error_message),
        );
        let sanitized_error = security::sanitize_error_message(&error_message);
        let _ =
            db_state.update_download_status(task_id, &DownloadStatus::Failed, Some(&sanitized_error));
        emit_download_error(&app, task_id, sanitized_error);
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

pub(super) fn process_next_pending(app: AppHandle) {
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
            _ => {
                // No more pending tasks, release the slot
                manager.release();
                break;
            }
        }
    }
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
