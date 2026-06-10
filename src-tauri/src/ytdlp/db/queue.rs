use super::Database;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use rusqlite::{params, OptionalExtension};

pub(super) fn map_download_row(row: &rusqlite::Row) -> rusqlite::Result<DownloadTaskInfo> {
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
        audio_format: row.get(14)?,
        audio_quality: row.get(15)?,
        error_detail: row.get(16)?,
        group_id: None,
        group_title: None,
    })
}

/// Like `map_download_row` but also reads `group_id` (col 17) and `group_title`
/// (col 18). Used by queries that LEFT JOIN download_groups for client-side grouping.
pub(super) fn map_download_row_with_group(
    row: &rusqlite::Row,
) -> rusqlite::Result<DownloadTaskInfo> {
    let mut task = map_download_row(row)?;
    task.group_id = row.get(17)?;
    task.group_title = row.get(18)?;
    Ok(task)
}

pub(super) const DOWNLOAD_COLUMNS: &str = "id, video_url, video_id, title, format_id, quality_label, output_path, status, progress, speed, eta, error_message, created_at, completed_at, audio_format, audio_quality, error_detail";
/// Same columns as DOWNLOAD_COLUMNS but qualified with the `d` alias and with
/// `d.group_id`, `g.title` appended — for queries joined to download_groups.
pub(super) const DOWNLOAD_COLUMNS_G: &str = "d.id, d.video_url, d.video_id, d.title, d.format_id, d.quality_label, d.output_path, d.status, d.progress, d.speed, d.eta, d.error_message, d.created_at, d.completed_at, d.audio_format, d.audio_quality, d.error_detail, d.group_id, g.title";

impl Database {
    pub fn insert_download(
        &self,
        req: &DownloadRequest,
        output_path: &str,
    ) -> Result<u64, AppError> {
        let conn = self.conn();
        let created_at = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO downloads (video_url, video_id, title, format_id, quality_label, output_path, created_at, audio_format, audio_quality)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                req.video_url,
                req.video_id,
                req.title,
                req.format_id,
                req.quality_label,
                output_path,
                created_at,
                req.audio_format,
                req.audio_quality,
            ],
        ).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(conn.last_insert_rowid() as u64)
    }

    pub fn update_download_status(
        &self,
        id: u64,
        status: &DownloadStatus,
        error_msg: Option<&str>,
        error_detail: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self.conn();

        conn.execute(
            "UPDATE downloads SET status = ?1, error_message = ?2, error_detail = ?3 WHERE id = ?4",
            params![status.to_string(), error_msg, error_detail, id],
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

    /// Remove only completed downloads from the queue. This matches the "Clear Completed"
    /// button label and its `completedCount === 0` disabled guard. Failed and cancelled rows
    /// are intentionally kept so users can inspect error messages and retry them.
    pub fn clear_completed(&self) -> Result<u32, AppError> {
        let conn = self.conn();

        let deleted = conn
            .execute("DELETE FROM downloads WHERE status = 'completed'", [])
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

        // Copy the downloads row's group_id into history so completed batch items
        // stay grouped in the history view (task id is the last param).
        tx.execute(
            "INSERT INTO history (video_url, video_id, title, quality_label, format, file_path, file_size, downloaded_at, group_id)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, group_id FROM downloads WHERE id = ?9",
            params![
                history.video_url,
                history.video_id,
                history.title,
                history.quality_label,
                history.format,
                history.file_path,
                history.file_size,
                history.downloaded_at,
                id,
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
    /// The task is built directly from the UPDATE's RETURNING row: a follow-up SELECT could
    /// fail after the row was already flipped, orphaning a claimed row no executor will run.
    pub fn claim_next_pending(&self) -> Result<Option<DownloadTaskInfo>, AppError> {
        let conn = self.conn();
        conn.query_row(
            &format!(
                "UPDATE downloads SET status = 'downloading'
                 WHERE id = (SELECT id FROM downloads WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1)
                 RETURNING {}",
                DOWNLOAD_COLUMNS
            ),
            [],
            map_download_row,
        )
        .optional()
        .map_err(|e| AppError::DatabaseError(e.to_string()))
    }

    /// Atomically claim a specific freshly enqueued task (pending -> downloading). Mirrors
    /// `claim_for_retry`: returns true only if this call performed the transition, so
    /// add_to_queue can neither double-dispatch a row a concurrent process_next_pending already
    /// claimed nor resurrect one cancelled in the insert -> claim window.
    pub fn claim_specific_pending(&self, id: u64) -> Result<bool, AppError> {
        let conn = self.conn();
        let rows = conn
            .execute(
                "UPDATE downloads SET status = 'downloading' WHERE id = ?1 AND status = 'pending'",
                params![id],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows > 0)
    }

    /// Atomically claim a specific task for retry by flipping it to 'downloading' only if it is
    /// currently in a retryable terminal/pending state. Returns true if this call claimed it.
    /// Using a single conditional UPDATE prevents a concurrent process_next_pending from
    /// double-dispatching the same task (which would spawn two yt-dlp processes / duplicate
    /// history rows).
    pub fn claim_for_retry(&self, id: u64) -> Result<bool, AppError> {
        let conn = self.conn();
        let rows = conn
            .execute(
                "UPDATE downloads
                 SET status = 'downloading', error_message = NULL, progress = 0.0, completed_at = NULL
                 WHERE id = ?1 AND status IN ('failed', 'cancelled', 'pending')",
                params![id],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows > 0)
    }

    /// Re-queue a retry only from retryable states. This is used when no execution slot is
    /// currently available, so the scheduler can claim the task later without resurrecting
    /// completed or already-running rows.
    pub fn queue_for_retry(&self, id: u64) -> Result<bool, AppError> {
        let conn = self.conn();
        let rows = conn
            .execute(
                "UPDATE downloads
                 SET status = 'pending', error_message = NULL, progress = 0.0, completed_at = NULL
                 WHERE id = ?1 AND status IN ('failed', 'cancelled', 'pending')",
                params![id],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows > 0)
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
                "UPDATE downloads SET status = 'failed', error_message = 'error.appClosedDuringDownload' WHERE status = 'downloading'",
                [],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows as u32)
    }

    /// Delete terminal queue rows older than `max_age_days` so the downloads table stays
    /// bounded; nothing else ever removes failed/cancelled rows. Completed rows are mirrored
    /// into history at completion time (complete_and_record), so pruning them loses nothing.
    /// download_groups rows are intentionally left alone — the history view JOINs them.
    pub fn prune_old_terminal_downloads(&self, max_age_days: u32) -> Result<u32, AppError> {
        let conn = self.conn();
        let cutoff = chrono::Utc::now().timestamp() - (max_age_days as i64 * 24 * 60 * 60);
        let rows = conn
            .execute(
                "DELETE FROM downloads WHERE status IN ('completed', 'failed', 'cancelled') AND created_at < ?1",
                params![cutoff],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows as u32)
    }

    pub fn get_queue_summary(&self, recent_completed_limit: u32) -> Result<QueueSummary, AppError> {
        let conn = self.conn();

        // Get status counts
        let mut count_stmt = conn
            .prepare("SELECT status, COUNT(*) FROM downloads GROUP BY status")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let mut active_count: u64 = 0;
        let mut pending_count: u64 = 0;
        let mut completed_count: u64 = 0;
        let mut total_count: u64 = 0;

        let rows = count_stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        for row in rows {
            let (status, count) = row.map_err(|e| AppError::DatabaseError(e.to_string()))?;
            total_count += count;
            match status.as_str() {
                "downloading" => active_count = count,
                "pending" => pending_count = count,
                "completed" => completed_count = count,
                _ => {}
            }
        }

        // Get active items (downloading + pending) with group title for popup chips.
        let mut active_stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads d LEFT JOIN download_groups g ON g.id = d.group_id WHERE d.status IN ('downloading', 'pending') ORDER BY d.created_at ASC",
                DOWNLOAD_COLUMNS_G
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let active_items = active_stmt
            .query_map([], map_download_row_with_group)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        // Get recent completed
        let mut completed_stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads WHERE status = 'completed' ORDER BY completed_at DESC LIMIT ?1",
                DOWNLOAD_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let recent_completed = completed_stmt
            .query_map(params![recent_completed_limit], map_download_row)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(QueueSummary {
            active_items,
            recent_completed,
            active_count,
            pending_count,
            completed_count,
            total_count,
        })
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

    /// Active queue for the unified view's "in progress" section: everything not yet completed
    /// (downloading, pending, failed, cancelled), newest first. Completed downloads live in the
    /// history table and are shown in the records section instead.
    /// Rows backing the queue page's status filters. Includes `completed` so the "Completed"
    /// filter tab can list finished downloads; the page keeps them out of the default view.
    ///
    /// All 'downloading'/'pending' rows are always returned unbounded (a global LIMIT could
    /// hide genuinely active items behind newer terminal rows); only terminal rows are capped
    /// at the newest ACTIVE_QUEUE_TERMINAL_LIMIT, since this query is polled every 5s and
    /// re-serialized over IPC in full. Note: the queue page's filter-tab counts are derived
    /// from this array, so terminal counts undercount once the cap is exceeded — acceptable
    /// because the startup prune (prune_old_terminal_downloads) keeps the table bounded.
    pub fn get_active_queue(&self) -> Result<Vec<DownloadTaskInfo>, AppError> {
        const ACTIVE_QUEUE_TERMINAL_LIMIT: u32 = 1000;

        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads d LEFT JOIN download_groups g ON g.id = d.group_id
                 WHERE d.id IN (
                     SELECT id FROM downloads WHERE status IN ('downloading', 'pending')
                     UNION ALL
                     SELECT id FROM (
                         SELECT id FROM downloads
                         WHERE status IN ('failed', 'cancelled', 'completed')
                         ORDER BY created_at DESC LIMIT {}
                     )
                 )
                 ORDER BY d.created_at DESC",
                DOWNLOAD_COLUMNS_G, ACTIVE_QUEUE_TERMINAL_LIMIT
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let tasks = stmt
            .query_map([], map_download_row_with_group)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(tasks)
    }

    /// Conditionally write a terminal status only while the row is still 'downloading'.
    /// Mirrors `cancel_if_active`: a stale executor winding down after a cancel, or a late
    /// panic guard, must never clobber a row another path already finalized (completed,
    /// cancelled, or re-queued as pending by a retry). Returns true if the row was updated.
    pub fn finalize_if_downloading(
        &self,
        id: u64,
        status: &DownloadStatus,
        error_msg: Option<&str>,
        error_detail: Option<&str>,
    ) -> Result<bool, AppError> {
        let conn = self.conn();
        let rows = conn
            .execute(
                "UPDATE downloads SET status = ?1, error_message = ?2, error_detail = ?3 WHERE id = ?4 AND status = 'downloading'",
                params![status.to_string(), error_msg, error_detail, id],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows > 0)
    }

    /// In-session analog of `reset_stale_downloads`: recover rows stuck in 'downloading' when
    /// no executor is running. A failed terminal-status write (disk full, transient SQLite
    /// error) leaves a finished attempt's row 'downloading', where Retry refuses to touch it.
    /// Callers must verify no execution slot is held before invoking this.
    pub fn fail_stuck_downloads(&self, error_msg: &str) -> Result<u32, AppError> {
        let conn = self.conn();
        let rows = conn
            .execute(
                "UPDATE downloads SET status = 'failed', error_message = ?1 WHERE status = 'downloading'",
                params![error_msg],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_row(db: &Database, status: &str, created_at: i64) -> i64 {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO downloads (video_url, video_id, title, format_id, quality_label, output_path, status, created_at)
             VALUES ('https://example.com/v', 'vid', 'title', 'fmt', '1080p', '/tmp', ?1, ?2)",
            params![status, created_at],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn prune_removes_only_old_terminal_rows() {
        let db = Database::new_in_memory().unwrap();
        let now = chrono::Utc::now().timestamp();
        let old = now - 40 * 24 * 60 * 60;

        let old_completed = insert_row(&db, "completed", old);
        let old_failed = insert_row(&db, "failed", old);
        let old_pending = insert_row(&db, "pending", old);
        let recent_failed = insert_row(&db, "failed", now);

        let pruned = db.prune_old_terminal_downloads(30).unwrap();
        assert_eq!(pruned, 2);

        let remaining: Vec<i64> = {
            let conn = db.conn();
            let mut stmt = conn.prepare("SELECT id FROM downloads").unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<i64>>>()
                .unwrap()
        };
        assert!(remaining.contains(&old_pending));
        assert!(remaining.contains(&recent_failed));
        assert!(!remaining.contains(&old_completed));
        assert!(!remaining.contains(&old_failed));
    }

    #[test]
    fn get_active_queue_never_drops_active_rows_beyond_terminal_cap() {
        let db = Database::new_in_memory().unwrap();
        let base = chrono::Utc::now().timestamp();

        // One old pending row buried under far more than the terminal cap of newer rows.
        let pending_id = insert_row(&db, "pending", base - 10_000);
        for i in 0..1100i64 {
            insert_row(&db, "failed", base + i);
        }

        let tasks = db.get_active_queue().unwrap();

        assert!(
            tasks.iter().any(|t| t.id as i64 == pending_id),
            "active row must never be hidden by the terminal cap"
        );
        let terminal_count = tasks
            .iter()
            .filter(|t| matches!(t.status, DownloadStatus::Failed))
            .count();
        assert_eq!(terminal_count, 1000);
    }
}
