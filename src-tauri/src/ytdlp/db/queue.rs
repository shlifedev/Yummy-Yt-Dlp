use super::Database;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use rusqlite::{params, OptionalExtension};

fn map_download_row(row: &rusqlite::Row) -> rusqlite::Result<DownloadTaskInfo> {
    Ok(DownloadTaskInfo {
        id: row.get(0)?,
        video_url: row.get(1)?,
        video_id: row.get(2)?,
        title: row.get(3)?,
        format_id: row.get(4)?,
        quality_label: row.get(5)?,
        output_path: row.get(6)?,
        status: DownloadStatus::parse(&row.get::<_, String>(7)?),
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

    /// Insert multiple downloads in a single transaction for batch/playlist operations.
    pub fn insert_downloads_batch(
        &self,
        items: &[(DownloadRequest, String)],
    ) -> Result<Vec<u64>, AppError> {
        let mut conn = self.conn();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let created_at = chrono::Utc::now().timestamp();
        let mut ids = Vec::with_capacity(items.len());

        for (req, output_path) in items {
            tx.execute(
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
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
            ids.push(tx.last_insert_rowid() as u64);
        }

        tx.commit()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(ids)
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

    /// Conditionally cancel a download only if it is still in a cancellable state.
    /// Returns true if the status was actually updated, false if the task was already
    /// completed/failed (preventing overwrite of a completed download's status).
    pub fn cancel_if_active(&self, id: u64) -> Result<bool, AppError> {
        let conn = self.conn();
        let rows_affected = conn
            .execute(
                "UPDATE downloads SET status = 'cancelled' WHERE id = ?1 AND status IN ('pending', 'downloading')",
                params![id],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows_affected > 0)
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
        // Scope the MutexGuard so it is dropped before calling get_download(),
        // which also acquires the same Mutex. std::sync::Mutex is non-reentrant,
        // so holding the guard while calling get_download() would deadlock.
        let claimed_id: Option<u64> = {
            let conn = self.conn();
            conn.query_row(
                "UPDATE downloads SET status = 'downloading'
                 WHERE id = (SELECT id FROM downloads WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1)
                 RETURNING id",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
        }; // MutexGuard dropped here

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

    /// Reset downloads that were left in 'downloading' state from a previous session.
    /// Called on app startup to clean up stale state after unexpected shutdown.
    pub fn reset_stale_downloads(&self) -> Result<u32, AppError> {
        let conn = self.conn();
        let rows = conn
            .execute(
                "UPDATE downloads SET status = 'failed', error_message = 'App closed during download' WHERE status = 'downloading'",
                [],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows as u32)
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
