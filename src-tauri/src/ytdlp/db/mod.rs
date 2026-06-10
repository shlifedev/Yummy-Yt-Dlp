mod groups;
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
const SCHEMA_VERSION: u32 = 8;

impl Database {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError> {
        std::fs::create_dir_all(app_data_dir).map_err(|e| {
            AppError::DatabaseError(format!("Failed to create app data dir: {}", e))
        })?;

        let db_path = app_data_dir.join("ytdlp.db");
        let conn =
            Connection::open(&db_path).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        // WAL + relaxed sync: far fewer fsyncs per write (the queue does frequent
        // progress/status updates) and readers don't block the single writer.
        // busy_timeout avoids spurious SQLITE_BUSY under brief contention.
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

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

        // Read the highest recorded version. Older builds used DELETE+INSERT (no fixed
        // rowid) so a row could exist at any rowid; MAX() reads it regardless and also
        // tolerates a stray duplicate left by an interrupted write.
        let version: Option<u32> = conn
            .query_row("SELECT MAX(version) FROM _schema_version", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .flatten();

        Ok(version.unwrap_or(0))
    }

    fn set_schema_version(conn: &Connection, version: u32) -> Result<(), AppError> {
        // Replace whatever rows exist with a single pinned row in one transaction. The
        // previous bare DELETE+INSERT left a window where _schema_version was empty; a
        // crash there reset the version to 0 and re-ran every migration. DELETE collapses
        // any duplicate rows older builds may have accumulated; rowid 1 keeps it canonical.
        conn.execute_batch(&format!(
            "BEGIN IMMEDIATE;
             DELETE FROM _schema_version;
             INSERT INTO _schema_version (rowid, version) VALUES (1, {});
             COMMIT;",
            version
        ))
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

        if current < 3 {
            // v3: Add indexes for queue pagination performance
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_downloads_completed_at ON downloads(completed_at);
                 CREATE INDEX IF NOT EXISTS idx_downloads_created_at ON downloads(created_at);",
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        if current < 4 {
            // v4: Add audio_format column for audio extraction (e.g. mp3)
            conn.execute_batch("ALTER TABLE downloads ADD COLUMN audio_format TEXT;")
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        if current < 5 {
            // v5: Add audio_quality column for audio bitrate/quality selection
            conn.execute_batch("ALTER TABLE downloads ADD COLUMN audio_quality TEXT;")
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        if current < 6 {
            // v6: Store the raw yt-dlp error line for failed downloads so the UI can show
            // the real cause instead of only the generic classified message.
            conn.execute_batch("ALTER TABLE downloads ADD COLUMN error_detail TEXT;")
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        if current < 7 {
            // v7: Download groups for batch playlist/channel downloads.
            // total_count is fixed at batch time so history headers can show
            // "N/total done" even after the queue rows are cleared.
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS download_groups (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    title TEXT NOT NULL,
                    kind TEXT NOT NULL DEFAULT 'playlist',
                    total_count INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL
                );
                ALTER TABLE downloads ADD COLUMN group_id INTEGER;
                ALTER TABLE history ADD COLUMN group_id INTEGER;
                CREATE INDEX IF NOT EXISTS idx_downloads_group_id ON downloads(group_id);
                CREATE INDEX IF NOT EXISTS idx_history_group_id ON history(group_id);",
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        if current < 8 {
            // v8: Repair installs stuck on v7. v7 bundled `ALTER TABLE history ADD COLUMN group_id`
            // into one execute_batch; where `downloads.group_id` already existed that batch aborted
            // early and left `history.group_id` missing, so every completed download failed to
            // record into history and duplicate detection never recognized anything. ensure_column
            // re-adds whatever is absent, idempotently.
            Self::ensure_column(conn, "downloads", "group_id", "INTEGER")?;
            Self::ensure_column(conn, "history", "group_id", "INTEGER")?;
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_downloads_group_id ON downloads(group_id);
                 CREATE INDEX IF NOT EXISTS idx_history_group_id ON history(group_id);",
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        if current < SCHEMA_VERSION {
            Self::set_schema_version(conn, SCHEMA_VERSION)?;
        }

        Ok(())
    }

    /// Add `column` to `table` only when it isn't already present. SQLite's `ALTER TABLE ADD
    /// COLUMN` errors on a duplicate column, which inside an `execute_batch` aborts every later
    /// statement — exactly how v7 left `history.group_id` missing on some installs. Checking first
    /// keeps the migration idempotent and safe to re-run.
    fn ensure_column(
        conn: &Connection,
        table: &str,
        column: &str,
        decl: &str,
    ) -> Result<(), AppError> {
        let present = {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({})", table))
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
            let names: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
                .collect::<rusqlite::Result<Vec<String>>>()
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
            names.iter().any(|name| name.as_str() == column)
        };

        if !present {
            conn.execute(
                &format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, decl),
                [],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }
        Ok(())
    }

    // Helper to handle Mutex poisoning gracefully
    pub(super) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Delete all data from downloads and history tables (used by factory reset).
    /// Uses the live connection instead of deleting DB files to avoid stale state.
    pub fn clear_all_data(&self) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute_batch(
            "DELETE FROM downloads; DELETE FROM history; DELETE FROM download_groups;",
        )
        .map_err(|e| AppError::DatabaseError(format!("Failed to clear database: {}", e)))?;
        // Reclaim disk space
        conn.execute_batch("VACUUM;")
            .map_err(|e| AppError::DatabaseError(format!("Failed to vacuum database: {}", e)))?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        conn.prepare(&format!("PRAGMA table_info({})", table))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<String>>>()
            .unwrap()
    }

    #[test]
    fn ensure_column_adds_when_missing_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE history (id INTEGER PRIMARY KEY, video_id TEXT NOT NULL);",
        )
        .unwrap();

        // First call adds the column; the second is a no-op (must not error on a duplicate column).
        Database::ensure_column(&conn, "history", "group_id", "INTEGER").unwrap();
        Database::ensure_column(&conn, "history", "group_id", "INTEGER").unwrap();

        assert!(column_names(&conn, "history")
            .iter()
            .any(|c| c == "group_id"));

        // Regression: a group_id-referencing insert (what complete_and_record does) must succeed.
        conn.execute(
            "INSERT INTO history (video_id, group_id) VALUES ('abc', 1)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn migration_repairs_v7_install_missing_history_group_id() {
        let conn = Connection::open_in_memory().unwrap();
        // Reproduce a v7 install where the batch added downloads.group_id but never reached
        // history.group_id — the exact state that broke history recording in the field.
        conn.execute_batch(
            "CREATE TABLE downloads (id INTEGER PRIMARY KEY, video_id TEXT NOT NULL, group_id INTEGER);
             CREATE TABLE history (id INTEGER PRIMARY KEY, video_id TEXT NOT NULL);
             CREATE TABLE _schema_version (version INTEGER NOT NULL);
             INSERT INTO _schema_version (version) VALUES (7);",
        )
        .unwrap();

        Database::run_migrations(&conn).unwrap();

        assert!(column_names(&conn, "history")
            .iter()
            .any(|c| c == "group_id"));
        assert_eq!(Database::get_schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }
}
