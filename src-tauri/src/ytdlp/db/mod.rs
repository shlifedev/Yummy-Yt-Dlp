mod history;
mod queue;

use crate::modules::types::AppError;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

/// Current schema version. Increment when adding new migrations.
const SCHEMA_VERSION: u32 = 2;

impl Database {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError> {
        std::fs::create_dir_all(app_data_dir).map_err(|e| {
            AppError::DatabaseError(format!("Failed to create app data dir: {}", e))
        })?;

        let db_path = app_data_dir.join("ytdlp.db");
        let conn =
            Connection::open(&db_path).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Self::create_tables(&conn)?;
        Self::run_migrations(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn get_schema_version(conn: &Connection) -> Result<u32, AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _schema_version (version INTEGER NOT NULL)",
            [],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let version: Option<u32> = conn
            .query_row("SELECT version FROM _schema_version LIMIT 1", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(version.unwrap_or(0))
    }

    fn set_schema_version(conn: &Connection, version: u32) -> Result<(), AppError> {
        conn.execute("DELETE FROM _schema_version", [])
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        conn.execute(
            "INSERT INTO _schema_version (version) VALUES (?1)",
            rusqlite::params![version],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    fn run_migrations(conn: &Connection) -> Result<(), AppError> {
        let current = Self::get_schema_version(conn)?;

        if current < 1 {
            // v1: Initial schema (tables already created by create_tables)
            // No additional migration needed for fresh installs
        }

        if current < 2 {
            // v2: Add indexes for performance
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_downloads_video_id ON downloads(video_id);
                 CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
                 CREATE INDEX IF NOT EXISTS idx_history_video_id ON history(video_id);
                 CREATE INDEX IF NOT EXISTS idx_history_downloaded_at ON history(downloaded_at);",
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        // Future migrations go here:
        // if current < 3 { ... }

        if current < SCHEMA_VERSION {
            Self::set_schema_version(conn, SCHEMA_VERSION)?;
        }

        Ok(())
    }

    // Helper to handle Mutex poisoning gracefully
    pub(super) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn create_tables(conn: &Connection) -> Result<(), AppError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS downloads (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                video_url TEXT NOT NULL,
                video_id TEXT NOT NULL,
                title TEXT NOT NULL,
                format_id TEXT NOT NULL,
                quality_label TEXT NOT NULL,
                output_path TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                progress REAL DEFAULT 0.0,
                speed TEXT,
                eta TEXT,
                error_message TEXT,
                created_at INTEGER NOT NULL,
                completed_at INTEGER
            )",
            [],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                video_url TEXT NOT NULL,
                video_id TEXT NOT NULL,
                title TEXT NOT NULL,
                quality_label TEXT NOT NULL,
                format TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_size INTEGER,
                downloaded_at INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}

use rusqlite::OptionalExtension;
