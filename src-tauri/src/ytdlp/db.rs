use super::types::*;
use crate::modules::types::AppError;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

pub struct Database {
    conn: Mutex<Connection>,
}

fn map_download_row(row: &rusqlite::Row) -> rusqlite::Result<DownloadTaskInfo> {
    Ok(DownloadTaskInfo {
        id: row.get(0)?,
        video_url: row.get(1)?,
        video_id: row.get(2)?,
        title: row.get(3)?,
        format_id: row.get(4)?,
        quality_label: row.get(5)?,
        output_path: row.get(6)?,
        status: DownloadStatus::from_str(&row.get::<_, String>(7)?),
        progress: row.get(8)?,
        speed: row.get(9)?,
        eta: row.get(10)?,
        error_message: row.get(11)?,
        created_at: row.get(12)?,
        completed_at: row.get(13)?,
    })
}

const DOWNLOAD_COLUMNS: &str = "id, video_url, video_id, title, format_id, quality_label, output_path, status, progress, speed, eta, error_message, created_at, completed_at";

impl Database {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError> {
        std::fs::create_dir_all(app_data_dir).map_err(|e| {
            AppError::DatabaseError(format!("Failed to create app data dir: {}", e))
        })?;

        let db_path = app_data_dir.join("ytdlp.db");
        let conn =
            Connection::open(&db_path).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Self::create_tables(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    // 1-5: Helper to handle Mutex poisoning gracefully
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
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

    pub fn insert_download(
        &self,
        req: &DownloadRequest,
        output_path: &str,
    ) -> Result<u64, AppError> {
        let conn = self.conn();
        let created_at = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO downloads (video_url, video_id, title, format_id, quality_label, output_path, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                req.video_url,
                req.video_id,
                req.title,
                req.format_id,
                req.quality_label,
                output_path,
                created_at,
            ],
        ).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn update_download_status(
        &self,
        id: u64,
        status: &DownloadStatus,
        error_msg: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self.conn();

        conn.execute(
            "UPDATE downloads SET status = ?1, error_message = ?2 WHERE id = ?3",
            params![status.to_string(), error_msg, id],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub fn update_download_progress(
        &self,
        id: u64,
        progress: f32,
        speed: Option<&str>,
        eta: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self.conn();

        conn.execute(
            "UPDATE downloads SET progress = ?1, speed = ?2, eta = ?3 WHERE id = ?4",
            params![progress, speed, eta, id],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub fn get_download_queue(&self) -> Result<Vec<DownloadTaskInfo>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads ORDER BY created_at DESC",
                DOWNLOAD_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let tasks = stmt
            .query_map([], map_download_row)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(tasks)
    }

    pub fn clear_completed(&self) -> Result<u32, AppError> {
        let conn = self.conn();

        let deleted = conn
            .execute(
                "DELETE FROM downloads WHERE status IN ('completed', 'cancelled', 'failed')",
                [],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(deleted as u32)
    }

    pub fn get_download(&self, id: u64) -> Result<Option<DownloadTaskInfo>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads WHERE id = ?1",
                DOWNLOAD_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let result = stmt.query_row([id], map_download_row);

        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::DatabaseError(e.to_string())),
        }
    }

    pub fn mark_completed(&self, id: u64, completed_at: i64) -> Result<(), AppError> {
        let conn = self.conn();

        conn.execute(
            "UPDATE downloads SET status = 'completed', completed_at = ?1, progress = 100.0 WHERE id = ?2",
            params![completed_at, id],
        ).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub fn complete_and_record(
        &self,
        id: u64,
        completed_at: i64,
        history: &HistoryItem,
    ) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        tx.execute(
            "UPDATE downloads SET status = 'completed', completed_at = ?1, progress = 100.0 WHERE id = ?2",
            params![completed_at, id],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        tx.execute(
            "INSERT INTO history (video_url, video_id, title, quality_label, format, file_path, file_size, downloaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                history.video_url,
                history.video_id,
                history.title,
                history.quality_label,
                history.format,
                history.file_path,
                history.file_size,
                history.downloaded_at,
            ],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        tx.commit()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub fn insert_history(&self, item: &HistoryItem) -> Result<u64, AppError> {
        let conn = self.conn();

        conn.execute(
            "INSERT INTO history (video_url, video_id, title, quality_label, format, file_path, file_size, downloaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                item.video_url,
                item.video_id,
                item.title,
                item.quality_label,
                item.format,
                item.file_path,
                item.file_size,
                item.downloaded_at,
            ],
        ).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn get_history(
        &self,
        page: u32,
        page_size: u32,
        search: Option<&str>,
    ) -> Result<HistoryResult, AppError> {
        let conn = self.conn();

        let (where_clause, search_param) = if let Some(s) = search {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            ("WHERE title LIKE ?1 ESCAPE '\\'", format!("%{}%", escaped))
        } else {
            ("", String::new())
        };

        let total_count: u64 = if search.is_some() {
            conn.query_row(
                &format!("SELECT COUNT(*) FROM history {}", where_clause),
                params![search_param],
                |row| row.get(0),
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
        } else {
            conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
        };

        let offset = page * page_size;
        let query = format!(
            "SELECT id, video_url, video_id, title, quality_label, format, file_path, file_size, downloaded_at
             FROM history
             {}
             ORDER BY downloaded_at DESC
             LIMIT ?{} OFFSET ?{}",
            where_clause,
            if search.is_some() { "2" } else { "1" },
            if search.is_some() { "3" } else { "2" }
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        fn map_row(row: &rusqlite::Row) -> rusqlite::Result<HistoryItem> {
            Ok(HistoryItem {
                id: row.get(0)?,
                video_url: row.get(1)?,
                video_id: row.get(2)?,
                title: row.get(3)?,
                quality_label: row.get(4)?,
                format: row.get(5)?,
                file_path: row.get(6)?,
                file_size: row.get(7)?,
                downloaded_at: row.get(8)?,
            })
        }

        let items = if search.is_some() {
            stmt.query_map(params![search_param, page_size, offset], map_row)
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
        } else {
            stmt.query_map(params![page_size, offset], map_row)
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
        };

        Ok(HistoryResult {
            items,
            total_count,
            page,
            page_size,
        })
    }

    pub fn check_duplicate_in_queue(&self, video_id: &str) -> Result<bool, AppError> {
        let conn = self.conn();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM downloads WHERE video_id = ?1 AND status IN ('pending', 'downloading')",
                [video_id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(count > 0)
    }

    pub fn check_duplicate(&self, video_id: &str) -> Result<Option<HistoryItem>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, video_url, video_id, title, quality_label, format, file_path, file_size, downloaded_at
             FROM history
             WHERE video_id = ?1
             ORDER BY downloaded_at DESC
             LIMIT 1"
        ).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let result = stmt.query_row([video_id], |row| {
            Ok(HistoryItem {
                id: row.get(0)?,
                video_url: row.get(1)?,
                video_id: row.get(2)?,
                title: row.get(3)?,
                quality_label: row.get(4)?,
                format: row.get(5)?,
                file_path: row.get(6)?,
                file_size: row.get(7)?,
                downloaded_at: row.get(8)?,
            })
        });

        match result {
            Ok(item) => Ok(Some(item)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::DatabaseError(e.to_string())),
        }
    }

    pub fn delete_history(&self, id: u64) -> Result<(), AppError> {
        let conn = self.conn();

        conn.execute("DELETE FROM history WHERE id = ?1", params![id])
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub fn get_next_pending(&self) -> Result<Option<DownloadTaskInfo>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1",
                DOWNLOAD_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let result = stmt.query_row([], map_download_row);

        match result {
            Ok(task) => Ok(Some(task)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::DatabaseError(e.to_string())),
        }
    }

    /// Atomically claim the next pending download by setting its status to 'downloading'
    /// in a single SQL statement. Returns the claimed task or None if no pending tasks exist.
    /// This prevents the race condition where two concurrent callers could claim the same task.
    pub fn claim_next_pending(&self) -> Result<Option<DownloadTaskInfo>, AppError> {
        let conn = self.conn();

        // Atomically update the oldest pending task to 'downloading' and return its id
        let claimed_id: Option<u64> = conn
            .query_row(
                "UPDATE downloads SET status = 'downloading'
                 WHERE id = (SELECT id FROM downloads WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1)
                 RETURNING id",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        match claimed_id {
            Some(id) => self.get_download(id),
            None => Ok(None),
        }
    }

    pub fn get_active_count(&self) -> Result<u32, AppError> {
        let conn = self.conn();
        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM downloads WHERE status = 'downloading'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(count)
    }

    pub fn get_cancellable_ids(&self) -> Result<Vec<u64>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id FROM downloads WHERE status IN ('downloading', 'pending')")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let ids = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<u64>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(ids)
    }

    pub fn get_active_downloads(&self) -> Result<Vec<DownloadTaskInfo>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads WHERE status IN ('downloading', 'pending') ORDER BY created_at ASC",
                DOWNLOAD_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let tasks = stmt
            .query_map([], map_download_row)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(tasks)
    }
}
