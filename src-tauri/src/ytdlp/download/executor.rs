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
/// On Unix, sends SIGKILL to the child's process group. Falls back to tokio child.kill().
/// Includes a timeout to prevent hanging if the process doesn't respond.
pub(crate) async fn kill_process_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(target_os = "windows")]
        {
            // taskkill /F (force) /T (tree - kill child processes) /PID
            let _ = tokio::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output()
                .await;
        }
        #[cfg(unix)]
        {
            // Send SIGKILL to the process group created for yt-dlp.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
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
fn emit_download_error(app: &AppHandle, task_id: u64, message: String, detail: Option<String>) {
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
            detail,
        },
    );
}

/// Record a terminal status for an attempt, but only while the row is still 'downloading'
/// (see `Database::finalize_if_downloading`): a stale executor or late panic guard must not
/// clobber a row another path already finalized. DB errors are logged and retried once instead
/// of being silently swallowed — the most common failure cause (full disk) tends to fail this
/// very write, leaving the row stuck 'downloading' with Retry refusing it. Returns true when
/// this call flipped the row, or when its state is unknown after a persistent DB failure, so
/// callers still report the failure instead of staying silent.
async fn set_terminal_status(
    db: &crate::DbState,
    task_id: u64,
    status: &DownloadStatus,
    error_msg: Option<&str>,
    error_detail: Option<&str>,
) -> bool {
    for attempt in 0..2u8 {
        match db.finalize_if_downloading(task_id, status, error_msg, error_detail) {
            Ok(flipped) => return flipped,
            Err(e) => {
                logger::error_cat(
                    "download",
                    &format!(
                        "[download:{}] failed to record terminal status '{}' (attempt {}): {}",
                        task_id,
                        status,
                        attempt + 1,
                        e
                    ),
                );
                if attempt == 0 {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }
    true
}

/// Helper: handle a fatal download error by logging, updating DB (only while the row is still
/// 'downloading'), emitting the event when the row flipped, and releasing the slot.
/// `error_msg` must be a stable i18n key; the dynamic cause goes in `detail`, which is
/// sanitized before persisting because the queue page renders error_detail verbatim.
async fn handle_download_failure(
    app: &AppHandle,
    task_id: u64,
    error_msg: &str,
    detail: Option<&str>,
    db: &crate::DbState,
    manager: &Arc<DownloadManager>,
    cancel_generation: u64,
) {
    let log_line = match detail {
        Some(d) => format!("[download:{}] {}: {}", task_id, error_msg, d),
        None => format!("[download:{}] {}", task_id, error_msg),
    };
    logger::error_cat("download", &log_line);
    let detail = detail.map(security::sanitize_error_message);
    if set_terminal_status(
        db,
        task_id,
        &DownloadStatus::Failed,
        Some(error_msg),
        detail.as_deref(),
    )
    .await
    {
        emit_download_error(app, task_id, error_msg.to_string(), detail);
    }
    manager.unregister_cancel(task_id, cancel_generation);
    manager.release();
    process_next_pending(app.clone());
}

/// Finalize a download whose executor task panicked: conditionally flip the row to 'failed'
/// (a late panic must not clobber a row already completed/cancelled/re-queued), emit the error
/// event when it flipped, drop the leaked cancel sender, then release the slot and dispatch
/// the next pending item. Without this the row stays 'downloading' for the session with Retry
/// silently refusing it. Public because the panic guard in commands/queue.rs cannot reach the
/// private helpers (same pattern as `process_next_pending_public`).
pub async fn finalize_panicked_download(app: AppHandle, task_id: u64, detail: String) {
    let db_state = app.state::<crate::DbState>();
    let manager = app.state::<Arc<DownloadManager>>();
    if set_terminal_status(
        &db_state,
        task_id,
        &DownloadStatus::Failed,
        Some("error.downloadFailed"),
        Some(&detail),
    )
    .await
    {
        emit_download_error(&app, task_id, "error.downloadFailed".to_string(), None);
    }
    manager.force_unregister_cancel(task_id);
    manager.release();
    process_next_pending(app);
}

pub(crate) fn append_limited(buffer: &mut String, line: &str, max_bytes: usize) {
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

/// Extract the resolved output file path from a yt-dlp stdout line, if the line announces one.
/// Handles the merge case, single-file/audio-extraction destinations, and the
/// `--no-overwrites` skip line ("<path> has already been downloaded") so a re-download of an
/// existing file records the real path instead of the unexpanded `%(title)s.%(ext)s` template.
pub(super) fn parse_output_destination(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("[Merger] Merging formats into \"") {
        return rest.strip_suffix('"').map(|p| p.to_string());
    }
    if let Some(rest) = line
        .strip_prefix("[download] Destination: ")
        .or_else(|| line.strip_prefix("[ExtractAudio] Destination: "))
    {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = line.strip_prefix("[download] ") {
        if let Some(path) = rest.strip_suffix(" has already been downloaded") {
            let path = path.trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

/// Whether `path` is a concrete file path yt-dlp resolved, not the unexpanded `%(...)s` output
/// template. Only resolved paths belong in history: duplicate detection stats the file on disk, and
/// a template like `%(title)s.%(ext)s` would never match anything there.
fn is_resolved_path(path: Option<&str>) -> bool {
    matches!(path, Some(p) if !p.is_empty() && !p.contains("%("))
}

/// Static directory prefix of an output template: every path component before the first one
/// containing a `%(` placeholder. Splitting the raw string at `%(` would truncate mid-component
/// for templates like `prefix%(title)s.%(ext)s`. A template with no placeholders at all is a
/// concrete file path, so its parent directory is the prefix.
pub(super) fn static_template_prefix(output_path: &str) -> std::path::PathBuf {
    let path = std::path::Path::new(output_path);
    let mut prefix = std::path::PathBuf::new();
    for component in path.components() {
        if component.as_os_str().to_string_lossy().contains("%(") {
            return prefix;
        }
        prefix.push(component);
    }
    path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

/// Nearest concrete ancestor directory of the output template, skipping path components that
/// are themselves templated (`%(uploader)s/%(title)s.%(ext)s` is a valid filename template).
/// Used as the history file_path when yt-dlp never announced a destination: the directory
/// exists on disk for a successful download, so duplicate detection's file stat keeps working,
/// while the raw template would never match anything.
pub(super) fn template_output_dir(output_path: &str) -> String {
    let mut dir = std::path::Path::new(output_path).parent();
    while let Some(d) = dir {
        if !d.to_string_lossy().contains("%(") {
            return d.to_string_lossy().to_string();
        }
        dir = d.parent();
    }
    String::new()
}

/// Map a finished yt-dlp process's exit code + stderr to a stable i18n error key.
/// Extracted as a pure function so the classification (cookie failures, network errors,
/// Windows cp949 encoding crashes) is testable. The frontend translates the key; the raw
/// stderr is logged separately, so it isn't embedded in the user-facing message anymore.
pub(super) fn classify_download_error(code: Option<i32>, stderr_output: &str) -> String {
    // "Could not copy" = cookie DB locked by a running browser; "cookies database" = yt-dlp's
    // lowercase "could not find <browser> cookies database in ..." when the configured browser
    // is uninstalled or its profile is unreadable (e.g. Safari without Full Disk Access).
    let is_cookie_error = (stderr_output.contains("Could not copy")
        && stderr_output.contains("cookie"))
        || stderr_output.contains("cookies database");
    let Some(code) = code else {
        return "error.processTerminated".to_string();
    };
    match code {
        1 => {
            if is_cookie_error {
                "error.cookieAccess".to_string()
            } else {
                "error.downloadFailed".to_string()
            }
        }
        // yt-dlp reserves exit code 2 for invalid user-provided options (optparse), e.g. an
        // outdated binary rejecting a flag. Network failures exit 1, not 2.
        2 => "error.invalidOptions".to_string(),
        120 => {
            let is_encoding_error = stderr_output.contains("cp949")
                || stderr_output.contains("cp932")
                || stderr_output.contains("TextIOWrapper")
                || stderr_output.contains("Errno 22")
                || stderr_output.contains("UnicodeEncodeError");
            if is_encoding_error {
                "error.encodingError".to_string()
            } else {
                "error.downloadFailed".to_string()
            }
        }
        _ => {
            if is_cookie_error {
                "error.cookieAccess".to_string()
            } else {
                "error.downloadFailed".to_string()
            }
        }
    }
}

/// Pull the most informative raw line out of yt-dlp's stderr so the UI can show the real cause
/// (e.g. "ERROR: Postprocessing: Error opening output files: Encoder not found") alongside the
/// generic classified key. Returns the last `ERROR:`-tagged line, falling back to the lowercase
/// optparse form ("yt-dlp: error: no such option: ..."), or None when neither is present.
pub(super) fn extract_ytdlp_error(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .rfind(|line| line.contains("ERROR:"))
        .or_else(|| {
            stderr
                .lines()
                .map(str::trim)
                .rfind(|line| line.contains("yt-dlp: error:"))
        })
        .map(str::to_string)
}

/// Public wrapper for execute_download (used by retry_download in commands.rs)
pub async fn execute_download_public(app: AppHandle, task_id: u64) {
    execute_download(app, task_id).await;
}

/// Outcome of a single yt-dlp download attempt, so the caller can decide whether to retry
/// (e.g. with `--impersonate` after an anti-bot 410) or finalize the download.
enum AttemptOutcome {
    Completed {
        file_path: Option<String>,
        already_existed: bool,
    },
    Failed {
        code: Option<i32>,
        stderr: String,
    },
    Cancelled,
    TimedOut,
    Fatal {
        msg: String,
    },
}

/// Run one yt-dlp download attempt: build the command from `args`, spawn it, stream progress to
/// the frontend, and wait with cancel/timeout support. Slot/cancel bookkeeping stays with the
/// caller so a single download can run more than one attempt (anti-bot impersonate fallback)
/// while registering its cancel receiver only once.
async fn run_download_attempt(
    app: &AppHandle,
    task_id: u64,
    ytdlp_path: &str,
    args: &[String],
    cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> AttemptOutcome {
    let db_state = app.state::<crate::DbState>();

    // Build command with augmented PATH including app bin dir
    let mut cmd = binary::command_with_path_app(ytdlp_path, app);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    // Final cancellation check before the point of no return. Binary/ffmpeg resolution can take
    // several seconds (cold yt-dlp start), and a cancel that arrived before register_cancel ran
    // would have been dropped (no receiver yet). cancel_if_active still set the DB row to
    // 'cancelled', so re-read it here and bail before spawning a file the user already cancelled.
    if let Ok(Some(t)) = db_state.get_download(task_id) {
        if matches!(t.status, DownloadStatus::Cancelled) {
            return AttemptOutcome::Cancelled;
        }
    }

    // Spawn process
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return AttemptOutcome::Fatal {
                msg: format!("Failed to spawn yt-dlp: {}", e),
            }
        }
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            return AttemptOutcome::Fatal {
                msg: "Failed to capture yt-dlp stdout".to_string(),
            }
        }
    };

    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            return AttemptOutcome::Fatal {
                msg: "Failed to capture yt-dlp stderr".to_string(),
            }
        }
    };

    // Clone necessary data for the async task
    let db_state_clone = db_state.inner().clone();
    let app_clone = app.clone();

    // Save JoinHandle for stdout reader task
    // Returns the actual output file path parsed from yt-dlp stdout
    let stdout_handle: tokio::task::JoinHandle<(Option<String>, bool)> = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let mut last_progress_percent: Option<f32> = None;
        let mut last_progress_update = tokio::time::Instant::now() - Duration::from_secs(1);
        // Persist progress to SQLite at most every few seconds (or on the final
        // tick). The frontend popup updates live from the IPC event above; the DB
        // row only backs the queue page's 2s poll, so it doesn't need 500ms writes.
        let mut last_db_progress_update = tokio::time::Instant::now() - Duration::from_secs(5);
        let mut actual_file_path: Option<String> = None;
        // yt-dlp prints "... has already been downloaded" and exits 0 when --no-overwrites finds
        // the file on disk. Track it so the caller records a skip instead of a fresh download.
        let mut already_existed = false;

        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) => break, // EOF
                Ok(_) => {}
                // EINTR is the one realistic transient error (tokio's read_until propagates
                // it, unlike std's). Anything else is a dead pipe — retrying busy-spins and,
                // since this handle is awaited unbounded after child.wait(), hangs the slot.
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    logger::warn_cat(
                        "download",
                        &format!(
                            "[download:{}] stdout read error, stopping reader: {}",
                            task_id, e
                        ),
                    );
                    break;
                }
            }
            let line = String::from_utf8_lossy(&buf).trim_end().to_string();
            // Capture the resolved output file path from yt-dlp's stdout (Destination/Merger
            // lines, plus the --no-overwrites "already been downloaded" skip line).
            if let Some(path) = parse_output_destination(&line) {
                actual_file_path = Some(path);
            }
            if line.contains(" has already been downloaded") {
                already_existed = true;
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
                        detail: None,
                    },
                );

                // Persist to DB on a slower cadence than the IPC emit; always
                // persist the terminal 100% tick so the queue page never shows a
                // stale sub-100 value for a finished item.
                if now.duration_since(last_db_progress_update) >= Duration::from_secs(5)
                    || progress_info.percent >= 100.0
                {
                    let _ = db_state_clone.update_download_progress(
                        task_id,
                        progress_info.percent,
                        Some(&speed),
                        Some(&eta),
                    );
                    last_db_progress_update = now;
                }

                last_progress_percent = Some(progress_info.percent);
                last_progress_update = now;
            }
        }

        (actual_file_path, already_existed)
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
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break, // persistent read error — no more useful data
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
                    // Kill before awaiting the readers: a live child holds the pipes open,
                    // so the unbounded awaits below would never complete — hanging this
                    // task forever while leaking the slot and the yt-dlp process.
                    kill_process_tree(&mut child).await;
                    let _ = stdout_handle.await;
                    let _ = stderr_handle.await;
                    return AttemptOutcome::Fatal {
                        msg: format!("Failed to wait for process: {}", e),
                    };
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
            return AttemptOutcome::TimedOut;
        }
        _ = cancel_rx.changed() => {
            // Cancel signal received - kill the yt-dlp process and its children (e.g., ffmpeg)
            kill_process_tree(&mut child).await;
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            return AttemptOutcome::Cancelled;
        }
    };

    // Await both stdout and stderr handles before checking result
    let (actual_file_path, already_existed) = stdout_handle.await.unwrap_or((None, false));
    let stderr_output = stderr_handle.await.unwrap_or_default();

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
        AttemptOutcome::Completed {
            file_path: actual_file_path,
            already_existed,
        }
    } else {
        AttemptOutcome::Failed {
            code: exit_code,
            stderr: stderr_output,
        }
    }
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

    // Register the cancel receiver up front, before any resolution work. Binary/ffmpeg
    // resolution can take several seconds on a cold yt-dlp start, and a cancel arriving in
    // that window used to be dropped (no receiver registered yet) while cancel_if_active had
    // already flipped the DB row to 'cancelled'. Registering here means send_cancel always has
    // a live receiver; run_download_attempt also re-reads the DB before spawning as a backstop.
    // The generation tags this attempt so a stale attempt can never unregister a newer one.
    let (cancel_generation, mut cancel_rx) = manager.register_cancel(task_id);

    let ytdlp_path = match binary::resolve_ytdlp_path_with_app(&app).await {
        Ok(p) => p,
        Err(e) => {
            // Same i18n key in the DB row and the emitted event so both surfaces agree.
            let error_msg = "error.ytdlpNotFound";
            logger::error_cat(
                "download",
                &format!("[download:{}] yt-dlp not found: {}", task_id, e),
            );
            let detail = security::sanitize_error_message(&e.to_string());
            if set_terminal_status(
                &db_state,
                task_id,
                &DownloadStatus::Failed,
                Some(error_msg),
                Some(&detail),
            )
            .await
            {
                emit_download_error(&app, task_id, error_msg.to_string(), Some(detail));
            }
            manager.unregister_cancel(task_id, cancel_generation);
            manager.release();
            process_next_pending(app);
            return;
        }
    };

    let settings = match settings::get_settings(&app) {
        Ok(s) => s,
        Err(e) => {
            let detail = format!("Failed to load settings: {}", e);
            handle_download_failure(
                &app,
                task_id,
                "error.downloadFailed",
                Some(&detail),
                &db_state,
                &manager,
                cancel_generation,
            )
            .await;
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
            detail: None,
        },
    );

    // cancel_rx was registered up front (before resolution) so a cancel during the cold-start
    // resolve window is not lost; it is shared across the impersonate retry below.

    // Defense-in-depth: validate the selector/audio fields before they reach argv. A queue row
    // could carry a tampered value (e.g. a DB poke or a future code path that skips request
    // validation), and a leading '-' or shell metacharacter there would be argument injection.
    let format_id = match security::sanitize_format_id(&task.format_id) {
        Ok(f) => f,
        Err(e) => {
            handle_download_failure(
                &app,
                task_id,
                "error.downloadFailed",
                Some(&format!("invalid format selector: {}", e)),
                &db_state,
                &manager,
                cancel_generation,
            )
            .await;
            return;
        }
    };
    let audio_format = match &task.audio_format {
        Some(fmt) => match security::sanitize_audio_format(fmt) {
            Ok(f) => Some(f),
            Err(e) => {
                handle_download_failure(
                    &app,
                    task_id,
                    "error.downloadFailed",
                    Some(&format!("invalid audio format: {}", e)),
                    &db_state,
                    &manager,
                    cancel_generation,
                )
                .await;
                return;
            }
        },
        None => None,
    };
    let audio_quality = match &task.audio_quality {
        Some(q) => match security::sanitize_audio_quality(q) {
            Ok(v) => Some(v),
            Err(e) => {
                handle_download_failure(
                    &app,
                    task_id,
                    "error.downloadFailed",
                    Some(&format!("invalid audio quality: {}", e)),
                    &db_state,
                    &manager,
                    cancel_generation,
                )
                .await;
                return;
            }
        },
        None => None,
    };

    // Preflight the download directory: if the configured folder lives on a disconnected
    // volume or offline network share, fail fast with a clear, retryable reason instead of
    // letting yt-dlp die with a generic exit-1 for every queued item.
    let static_dir = static_template_prefix(&task.output_path);
    if !static_dir.as_os_str().is_empty() {
        if let Err(e) = tokio::fs::create_dir_all(&static_dir).await {
            handle_download_failure(
                &app,
                task_id,
                "error.downloadPathUnavailable",
                Some(&format!("{}: {}", static_dir.display(), e)),
                &db_state,
                &manager,
                cancel_generation,
            )
            .await;
            return;
        }
    }

    // Build yt-dlp args in a Vec for logging before passing to Command
    let mut args: Vec<String> = Vec::new();
    args.extend(["--format".to_string(), format_id]);
    args.extend(["--output".to_string(), task.output_path.clone()]);
    args.extend([
        "--progress-template".to_string(),
        progress::progress_template(),
    ]);
    args.push("--newline".to_string());
    args.push("--no-playlist".to_string());
    args.push("--no-overwrites".to_string());

    // Add audio extraction flags if audio_format is specified (e.g. mp3, flac, opus, wav).
    // Both values were validated above.
    if let Some(audio_fmt) = &audio_format {
        args.push("--extract-audio".to_string());
        args.extend(["--audio-format".to_string(), audio_fmt.clone()]);
        if let Some(quality) = &audio_quality {
            args.extend(["--audio-quality".to_string(), quality.clone()]);
        }
    }

    // Force UTF-8 encoding inside yt-dlp (fixes cp949 crash on Korean Windows)
    args.push("--encoding".to_string());
    args.push("UTF-8".to_string());

    // Sanitize filenames for Windows forbidden characters
    #[cfg(target_os = "windows")]
    {
        args.push("--windows-filenames".to_string());
    }

    // Pass ffmpeg location explicitly if available. Capture availability so advanced options can
    // skip ffmpeg-dependent flags (embedding, remux, sponsorblock-remove, etc.).
    let ffmpeg_available = match binary::resolve_ffmpeg_path_with_app(&app).await {
        Some(ffmpeg_path) => {
            args.extend(["--ffmpeg-location".to_string(), ffmpeg_path]);
            true
        }
        None => false,
    };

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

    // Apply global advanced options (subtitles, SponsorBlock, embedding, codec, network, etc.)
    args.extend(super::advanced::build_advanced_args(
        &settings.advanced,
        audio_format.is_some(),
        ffmpeg_available,
    ));

    // End option parsing before the positional URL so a URL starting with '-' (or any future
    // value) can never be interpreted as a yt-dlp flag.
    args.push("--".to_string());
    // Add video URL
    args.push(task.video_url.clone());

    // Log the full command before spawning
    logger::info_cat(
        "download",
        &format!("[download:{}] spawning: {} {:?}", task_id, ytdlp_path, args),
    );

    // First attempt without impersonation.
    let mut outcome = run_download_attempt(&app, task_id, &ytdlp_path, &args, &mut cancel_rx).await;

    // Anti-bot (410/403) auto-fallback: retry once with --impersonate. The block happens during
    // extraction before any file is written, and --no-overwrites keeps the retry safe.
    if let AttemptOutcome::Failed {
        code: first_code,
        stderr: first_stderr,
    } = &outcome
    {
        if crate::ytdlp::metadata::looks_like_antibot_block(first_stderr) {
            let first_code = *first_code;
            let first_stderr = first_stderr.clone();
            logger::warn_cat(
                "download",
                &format!(
                    "[download:{}] anti-bot block (410/403), retrying with --impersonate",
                    task_id
                ),
            );
            // Insert before the trailing `-- <url>` pair; appended after `--` these would be
            // parsed as URLs instead of flags.
            let sep = args.len() - 2;
            args.insert(sep, "--impersonate".to_string());
            args.insert(
                sep + 1,
                crate::ytdlp::metadata::IMPERSONATE_TARGET.to_string(),
            );
            outcome = run_download_attempt(&app, task_id, &ytdlp_path, &args, &mut cancel_rx).await;

            // If the retry only proved that this yt-dlp doesn't know --impersonate (optparse
            // exits 2 with lowercase "yt-dlp: error: no such option"), classifying it would
            // report the wrong cause. Keep attempt 1's real outcome and append the optparse
            // line so the outdated binary is still visible in the logged stderr.
            if let AttemptOutcome::Failed { code, stderr } = &outcome {
                if *code == Some(2) || stderr.contains("no such option") {
                    logger::warn_cat(
                        "download",
                        &format!(
                            "[download:{}] --impersonate unsupported by this yt-dlp (update it); keeping first attempt's failure",
                            task_id
                        ),
                    );
                    let combined = match extract_ytdlp_error(stderr) {
                        Some(line) => format!("{}\n{}", first_stderr, line),
                        None => first_stderr,
                    };
                    outcome = AttemptOutcome::Failed {
                        code: first_code,
                        stderr: combined,
                    };
                }
            }
        }
    }

    match outcome {
        AttemptOutcome::Completed {
            file_path,
            already_existed,
        } => {
            // Trust the path yt-dlp actually reported. When no Destination/Merger line was
            // parsed from stdout (yt-dlp wording drift, --quiet via a user config), fall back
            // to the template's parent directory — never the raw template: a templated path
            // must not reach history (duplicate detection stats the file on disk and
            // "%(title)s.%(ext)s" can't be found), while the directory genuinely exists for a
            // successful download. file_size stays unknown (None) instead of a bogus 0.
            let resolved_path = file_path;
            let path_is_resolved = is_resolved_path(resolved_path.as_deref());
            let (event_path, file_size) = match &resolved_path {
                Some(p) if path_is_resolved => {
                    let size = tokio::fs::metadata(p).await.ok().map(|m| m.len());
                    (p.clone(), size)
                }
                _ => {
                    logger::warn_cat(
                        "download",
                        &format!(
                            "[download:{}] no destination line parsed from yt-dlp stdout; recording output directory instead",
                            task_id
                        ),
                    );
                    (template_output_dir(&task.output_path), None)
                }
            };

            let completed_at = chrono::Utc::now().timestamp();

            // A fresh download always records history. A --no-overwrites skip (already_existed)
            // usually relies on the original download's history row — but that row can be missing
            // (e.g. a past migration failure left history empty), and then the same video keeps
            // getting re-queued and re-spawned forever. So when the file already existed, still
            // record it if history has no row for this video and we know the real on-disk path.
            let record_history = if already_existed {
                path_is_resolved && matches!(db_state.check_duplicate(&task.video_id), Ok(None))
            } else {
                true
            };

            if record_history {
                let history_item = HistoryItem {
                    id: 0,
                    video_url: task.video_url.clone(),
                    video_id: task.video_id.clone(),
                    title: task.title.clone(),
                    quality_label: task.quality_label.clone(),
                    format: task.format_id.clone(),
                    file_path: event_path.clone(),
                    file_size,
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
                    // Fallback: mark completed and best-effort insert the history row separately so
                    // the download still shows in History and duplicate detection keeps recognizing
                    // it (otherwise the queue says 'completed' but the video looks never-downloaded).
                    // Prefer the group-preserving insert (a plain insert drops group_id and the
                    // batch header undercounts forever); if it hits the same failure as
                    // complete_and_record, fall back to the plain insert so the row is never lost.
                    let _ = db_state.mark_completed(task_id, completed_at);
                    if let Err(e2) = db_state
                        .insert_history_with_group(&history_item, task_id)
                        .or_else(|_| db_state.insert_history(&history_item))
                    {
                        logger::error_cat(
                            "download",
                            &format!(
                                "[download:{}] fallback insert_history also failed: {}",
                                task_id, e2
                            ),
                        );
                    }
                }

                logger::info_cat(
                    "download",
                    &format!(
                        "[download:{}] completed successfully ({}), file_size={:?}",
                        task_id,
                        if already_existed {
                            "existing file recorded"
                        } else {
                            "new download"
                        },
                        file_size
                    ),
                );
            } else {
                // File already on disk and either already in history or its path is unknown:
                // just finalize the queue row, no history change.
                let _ = db_state.mark_completed(task_id, completed_at);
                logger::info_cat(
                    "download",
                    &format!(
                        "[download:{}] file already exists, marked completed",
                        task_id
                    ),
                );
            }

            let _ = app.emit(
                "download-event",
                GlobalDownloadEvent {
                    task_id,
                    event_type: "completed".to_string(),
                    percent: Some(100.0),
                    speed: None,
                    eta: None,
                    file_path: Some(event_path),
                    file_size,
                    message: None,
                    detail: None,
                },
            );
        }
        AttemptOutcome::Failed { code, stderr } => {
            let error_message = classify_download_error(code, &stderr);
            // The classified key is a stable, translatable summary; keep the real yt-dlp ERROR
            // line as detail so the user sees the actual cause, not just "Download failed".
            let error_detail =
                extract_ytdlp_error(&stderr).map(|d| security::sanitize_error_message(&d));

            // Log full error internally, sanitize for frontend
            logger::error_cat(
                "download",
                &format!("[download:{}] failed: {}", task_id, error_message),
            );
            let sanitized_error = security::sanitize_error_message(&error_message);
            if set_terminal_status(
                &db_state,
                task_id,
                &DownloadStatus::Failed,
                Some(&sanitized_error),
                error_detail.as_deref(),
            )
            .await
            {
                emit_download_error(&app, task_id, sanitized_error, error_detail);
            }
        }
        AttemptOutcome::Cancelled => {
            // Conditional on purpose: in a plain cancel the row is already 'cancelled'
            // (no-op), and a retry re-queued as 'pending' in the kill window must not be
            // stomped back to 'cancelled' by this stale attempt.
            set_terminal_status(&db_state, task_id, &DownloadStatus::Cancelled, None, None).await;
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
                    message: Some("error.downloadCancelled".to_string()),
                    detail: None,
                },
            );
        }
        AttemptOutcome::TimedOut => {
            let error_msg = "error.downloadTimeout";
            if set_terminal_status(
                &db_state,
                task_id,
                &DownloadStatus::Failed,
                Some(error_msg),
                None,
            )
            .await
            {
                emit_download_error(&app, task_id, error_msg.to_string(), None);
            }
        }
        AttemptOutcome::Fatal { msg } => {
            // handle_download_failure already releases the slot and dispatches the next task.
            handle_download_failure(
                &app,
                task_id,
                "error.downloadFailed",
                Some(&msg),
                &db_state,
                &manager,
                cancel_generation,
            )
            .await;
            return;
        }
    }

    // Release the download slot and process next pending
    manager.unregister_cancel(task_id, cancel_generation);
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
                        finalize_panicked_download(
                            app_panic_guard,
                            task_id,
                            format!("internal panic: {:?}", e),
                        )
                        .await;
                    }
                });
            }
            Ok(None) => {
                // No more pending tasks, release the slot
                manager.release();
                break;
            }
            Err(e) => {
                // A transient DB error here would otherwise stall the queue silently: nothing
                // re-kicks the scheduler until unrelated queue activity or an app restart.
                logger::error_cat(
                    "download",
                    &format!("failed to claim next pending download: {}", e),
                );
                manager.release();
                break;
            }
        }
    }

    // Opportunistic self-heal: when no execution slot is held, any row still 'downloading' is
    // provably stuck (its terminal-status write failed, e.g. on a full disk). Retry refuses
    // such rows, so without this only an app restart would repair them.
    if manager.active_count() == 0 {
        match db_state.fail_stuck_downloads("error.processTerminated") {
            Ok(0) => {}
            Ok(n) => logger::warn_cat(
                "download",
                &format!(
                    "reset {} stuck 'downloading' row(s) with no live executor",
                    n
                ),
            ),
            Err(e) => logger::error_cat(
                "download",
                &format!("failed to reset stuck downloads: {}", e),
            ),
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

    #[test]
    fn parse_destination_lines() {
        assert_eq!(
            parse_output_destination("[download] Destination: /tmp/video.mp4"),
            Some("/tmp/video.mp4".to_string())
        );
        assert_eq!(
            parse_output_destination("[ExtractAudio] Destination: /tmp/song.mp3"),
            Some("/tmp/song.mp3".to_string())
        );
        assert_eq!(
            parse_output_destination("[Merger] Merging formats into \"/tmp/video.mkv\""),
            Some("/tmp/video.mkv".to_string())
        );
    }

    #[test]
    fn classify_cookie_failure_on_code_1() {
        let msg = classify_download_error(
            Some(1),
            "ERROR: Could not copy Chrome cookie database. Permission denied.",
        );
        assert_eq!(msg, "error.cookieAccess");
    }

    #[test]
    fn classify_code_1_empty_stderr_is_generic() {
        assert_eq!(classify_download_error(Some(1), ""), "error.downloadFailed");
    }

    #[test]
    fn parse_already_downloaded_line() {
        assert_eq!(
            parse_output_destination("[download] /tmp/video.mp4 has already been downloaded"),
            Some("/tmp/video.mp4".to_string())
        );
    }

    #[test]
    fn parse_ignores_unrelated_lines() {
        assert_eq!(parse_output_destination("[download]  12.3% of 5MiB"), None);
        assert_eq!(parse_output_destination("[info] Writing thumbnail"), None);
    }

    #[test]
    fn template_output_dir_returns_parent() {
        assert_eq!(
            template_output_dir("/Users/x/Downloads/%(title)s.%(ext)s"),
            "/Users/x/Downloads"
        );
    }

    #[test]
    fn template_output_dir_skips_templated_components() {
        assert_eq!(
            template_output_dir("/Users/x/Downloads/%(uploader)s/%(title)s.%(ext)s"),
            "/Users/x/Downloads"
        );
    }

    #[test]
    fn classify_code_1_returns_generic_key() {
        let msg =
            classify_download_error(Some(1), "warning: something\n\nERROR: video unavailable");
        assert_eq!(msg, "error.downloadFailed");
    }

    #[test]
    fn classify_invalid_options_on_code_2() {
        // yt-dlp reserves exit code 2 for optparse failures (e.g. an outdated binary
        // rejecting --impersonate); network errors raise DownloadError and exit 1.
        let msg = classify_download_error(Some(2), "yt-dlp: error: no such option: --impersonate");
        assert_eq!(msg, "error.invalidOptions");
    }

    #[test]
    fn classify_missing_cookies_database_on_code_1() {
        let msg = classify_download_error(
            Some(1),
            "ERROR: could not find chrome cookies database in \"~/Library/Application Support/Google/Chrome\"",
        );
        assert_eq!(msg, "error.cookieAccess");
    }

    #[test]
    fn classify_missing_cookies_database_on_unknown_code() {
        let msg =
            classify_download_error(Some(99), "ERROR: could not find firefox cookies database");
        assert_eq!(msg, "error.cookieAccess");
    }

    #[test]
    fn classify_code_120_encoding_crash() {
        let msg =
            classify_download_error(Some(120), "UnicodeEncodeError: 'cp949' codec can't encode");
        assert_eq!(msg, "error.encodingError");
    }

    #[test]
    fn classify_code_120_non_encoding_is_generic() {
        let msg = classify_download_error(Some(120), "some other failure");
        assert_eq!(msg, "error.downloadFailed");
    }

    #[test]
    fn classify_unknown_code_is_generic() {
        let msg = classify_download_error(Some(99), "boom");
        assert_eq!(msg, "error.downloadFailed");
    }

    #[test]
    fn classify_no_exit_code_is_unexpected_termination() {
        let msg = classify_download_error(None, "killed");
        assert_eq!(msg, "error.processTerminated");
    }

    #[test]
    fn extract_ytdlp_error_returns_last_error_line() {
        let stderr = "[download] 100% of 5MiB\nERROR: Postprocessing: Error opening output files: Encoder not found";
        assert_eq!(
            extract_ytdlp_error(stderr).as_deref(),
            Some("ERROR: Postprocessing: Error opening output files: Encoder not found")
        );
    }

    #[test]
    fn extract_ytdlp_error_none_when_no_error_line() {
        assert_eq!(
            extract_ytdlp_error("[download] just progress\nfinished"),
            None
        );
    }

    #[test]
    fn extract_ytdlp_error_falls_back_to_optparse_line() {
        let stderr = "Usage: yt-dlp [OPTIONS] URL\n\nyt-dlp: error: no such option: --impersonate";
        assert_eq!(
            extract_ytdlp_error(stderr).as_deref(),
            Some("yt-dlp: error: no such option: --impersonate")
        );
    }

    #[test]
    fn extract_ytdlp_error_prefers_uppercase_error_line() {
        let stderr =
            "ERROR: HTTP Error 403: Forbidden\nyt-dlp: error: no such option: --impersonate";
        assert_eq!(
            extract_ytdlp_error(stderr).as_deref(),
            Some("ERROR: HTTP Error 403: Forbidden")
        );
    }

    #[test]
    fn static_prefix_stops_before_placeholder_component() {
        assert_eq!(
            static_template_prefix("/Users/me/Downloads/%(title)s.%(ext)s"),
            std::path::PathBuf::from("/Users/me/Downloads")
        );
        // The placeholder can start mid-component; a raw split at "%(" would keep "prefix".
        assert_eq!(
            static_template_prefix("/Users/me/Downloads/prefix%(title)s.%(ext)s"),
            std::path::PathBuf::from("/Users/me/Downloads")
        );
        assert_eq!(
            static_template_prefix("/Users/me/Downloads/%(uploader)s/%(title)s.%(ext)s"),
            std::path::PathBuf::from("/Users/me/Downloads")
        );
    }

    #[test]
    fn static_prefix_of_concrete_path_is_its_parent() {
        assert_eq!(
            static_template_prefix("/Users/me/Downloads/video.mp4"),
            std::path::PathBuf::from("/Users/me/Downloads")
        );
    }
}
