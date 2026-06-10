use chrono::Local;
use std::fs::{self, create_dir_all, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock};

use super::log_db::LogDatabase;

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_DB: OnceLock<Arc<LogDatabase>> = OnceLock::new();
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Maximum log file size before rotation (5 MB)
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

/// A single log record handed off to the background writer thread.
struct LogRecord {
    timestamp_millis: i64,
    formatted_time: String,
    level: String,
    category: String,
    message: String,
    details: Option<String>,
}

/// Unbounded channel to the background writer. Lazily started on first log so callers never
/// block on file I/O, the SQLite mutex, or event emission in an async context.
static LOG_TX: OnceLock<Sender<LogRecord>> = OnceLock::new();

/// Lazily start (once) the background writer thread and return the sender.
fn writer_tx() -> &'static Sender<LogRecord> {
    LOG_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<LogRecord>();
        std::thread::Builder::new()
            .name("log-writer".to_string())
            .spawn(move || {
                // Drains until every Sender is dropped (process shutdown). All file rotation,
                // file appends, DB inserts, and live-update emits happen here, off the hot path.
                for rec in rx {
                    write_record(&rec);
                }
            })
            .expect("failed to spawn log writer thread");
        tx
    })
}

/// Perform the actual side effects for one record on the writer thread.
fn write_record(rec: &LogRecord) {
    // 1. File logging (crash fallback)
    if let Some(log_path) = LOG_PATH.get() {
        if let Some(parent) = log_path.parent() {
            let _ = create_dir_all(parent);
        }
        maybe_rotate(log_path);

        let log_entry = format!(
            "[{}] [{}] [{}] {}\n",
            rec.formatted_time, rec.level, rec.category, rec.message
        );

        match OpenOptions::new().create(true).append(true).open(log_path) {
            Ok(mut file) => {
                let _ = file.write_all(log_entry.as_bytes());
            }
            Err(e) => {
                eprintln!("[Logger] Failed to write log: {}", e);
            }
        }

        eprint!("{}", log_entry);
    }

    // 2. DB logging + 3. event emission for live updates
    if let Some(db) = LOG_DB.get() {
        if let Ok(id) = db.insert_log(
            rec.timestamp_millis,
            &rec.level,
            &rec.category,
            &rec.message,
            rec.details.as_deref(),
        ) {
            if let Some(app) = APP_HANDLE.get() {
                use tauri::Emitter;
                let entry = crate::ytdlp::types::LogEntry {
                    id,
                    timestamp: rec.timestamp_millis,
                    level: rec.level.clone(),
                    category: rec.category.clone(),
                    message: rec.message.clone(),
                    details: rec.details.clone(),
                };
                let _ = app.emit("new-log-event", crate::ytdlp::types::NewLogEvent { entry });
            }
        }
    }
}

/// Initialize the logger with the app data directory
pub fn init(app_data_dir: PathBuf) {
    let log_path = app_data_dir.join("log.txt");
    let _ = LOG_PATH.set(log_path);
}

/// Initialize the log database for structured logging
pub fn init_db(log_db: Arc<LogDatabase>) {
    let _ = LOG_DB.set(log_db);
}

/// Initialize the app handle for event emission
pub fn init_app_handle(handle: tauri::AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

/// Get the log file path
fn get_log_path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}

/// Rotate log file if it exceeds MAX_LOG_SIZE.
/// Renames current log to log.old.txt (overwriting previous backup).
fn maybe_rotate(log_path: &PathBuf) {
    if let Ok(meta) = fs::metadata(log_path) {
        if meta.len() > MAX_LOG_SIZE {
            let old_path = log_path.with_extension("old.txt");
            let _ = fs::rename(log_path, old_path);
        }
    }
}

/// Core log function with category support. Formats the timestamp on the caller's thread
/// (cheap) and hands the record to the background writer; all I/O happens off this thread.
fn write_log_with_category(level: &str, category: &str, message: &str, details: Option<&str>) {
    let now = Local::now();
    let timestamp_millis = chrono::Utc::now().timestamp_millis();

    if LOG_PATH.get().is_none() && LOG_DB.get().is_none() {
        eprintln!(
            "[Logger] Not initialized: [{}] [{}] {}",
            level, category, message
        );
        return;
    }

    let record = LogRecord {
        timestamp_millis,
        formatted_time: now.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        level: level.to_string(),
        category: category.to_string(),
        message: message.to_string(),
        details: details.map(|s| s.to_string()),
    };

    // Send is non-blocking on an unbounded channel. A failure only happens if the writer
    // thread died, in which case fall back to stderr so the line isn't silently lost.
    if writer_tx().send(record).is_err() {
        eprintln!(
            "[Logger] writer thread unavailable: [{}] [{}] {}",
            level, category, message
        );
    }
}

/// Write a log entry (backward compatible - uses "app" category)
fn write_log(level: &str, message: &str) {
    write_log_with_category(level, "app", message, None);
}

// === Backward-compatible functions (category defaults to "app") ===

/// Log an error message
pub fn error(message: &str) {
    write_log("ERROR", message);
}

/// Log an error with context
pub fn error_with_context(context: &str, message: &str) {
    write_log("ERROR", &format!("[{}] {}", context, message));
}

/// Log a warning message
pub fn warn(message: &str) {
    write_log("WARN", message);
}

/// Log an info message
pub fn info(message: &str) {
    write_log("INFO", message);
}

// === Category-aware functions ===

/// Log with explicit category and optional details
pub fn log(level: &str, category: &str, message: &str, details: Option<&str>) {
    write_log_with_category(level, category, message, details);
}

pub fn info_cat(category: &str, message: &str) {
    write_log_with_category("INFO", category, message, None);
}

pub fn error_cat(category: &str, message: &str) {
    write_log_with_category("ERROR", category, message, None);
}

pub fn warn_cat(category: &str, message: &str) {
    write_log_with_category("WARN", category, message, None);
}

pub fn debug_cat(category: &str, message: &str) {
    write_log_with_category("DEBUG", category, message, None);
}

/// Read the last N lines from the log file using tail-style reading.
/// Only reads up to 256 KB from the end of the file to avoid loading huge files.
pub fn read_recent_logs(max_lines: usize) -> String {
    let Some(log_path) = get_log_path() else {
        return "Logger not initialized".to_string();
    };

    let mut file = match fs::File::open(log_path) {
        Ok(f) => f,
        Err(e) => return format!("Failed to read log file: {}", e),
    };

    let file_len = match file.metadata() {
        Ok(m) => m.len(),
        Err(e) => return format!("Failed to read log metadata: {}", e),
    };

    // Read at most 256 KB from the end
    let read_size = file_len.min(256 * 1024);
    let start_pos = file_len - read_size;

    if let Err(e) = file.seek(SeekFrom::Start(start_pos)) {
        return format!("Failed to seek log file: {}", e);
    }

    let mut buf = vec![0u8; read_size as usize];
    if let Err(e) = file.read_exact(&mut buf) {
        return format!("Failed to read log tail: {}", e);
    }

    let content = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}
