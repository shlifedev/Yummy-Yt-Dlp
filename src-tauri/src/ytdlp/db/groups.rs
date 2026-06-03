use super::Database;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use rusqlite::{params, OptionalExtension};

const HISTORY_COLUMNS: &str =
    "id, video_url, video_id, title, quality_label, format, file_path, file_size, downloaded_at";

struct StatusCounts {
    active: u64,
    pending: u64,
    completed: u64,
    failed: u64,
    cancelled: u64,
}

fn read_status_counts(conn: &rusqlite::Connection) -> Result<StatusCounts, AppError> {
    let mut stmt = conn
        .prepare("SELECT status, COUNT(*) FROM downloads GROUP BY status")
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
    let mut counts = StatusCounts {
        active: 0,
        pending: 0,
        completed: 0,
        failed: 0,
        cancelled: 0,
    };
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
    for row in rows {
        let (status, count) = row.map_err(|e| AppError::DatabaseError(e.to_string()))?;
        match status.as_str() {
            "downloading" => counts.active = count,
            "pending" => counts.pending = count,
            "completed" => counts.completed = count,
            "failed" => counts.failed = count,
            "cancelled" => counts.cancelled = count,
            _ => {}
        }
    }
    Ok(counts)
}

impl Database {
    /// Insert a batch of downloads, optionally wrapped in a group, in one transaction.
    /// A group row is created only when there are 2+ items; otherwise items go in
    /// standalone (group_id = NULL) and the returned group_id is None.
    pub fn insert_group_with_downloads(
        &self,
        title: Option<&str>,
        kind: &str,
        items: &[(DownloadRequest, String)],
    ) -> Result<(Option<u64>, Vec<u64>), AppError> {
        let mut conn = self.conn();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let created_at = chrono::Utc::now().timestamp();

        // Only wrap in a group when a title is given AND there are 2+ items.
        let group_id: Option<u64> = match (title, items.len() >= 2) {
            (Some(t), true) => {
                tx.execute(
                    "INSERT INTO download_groups (title, kind, total_count, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![t, kind, items.len() as i64, created_at],
                )
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                Some(tx.last_insert_rowid() as u64)
            }
            _ => None,
        };

        let mut ids = Vec::with_capacity(items.len());
        for (req, output_path) in items {
            tx.execute(
                "INSERT INTO downloads (video_url, video_id, title, format_id, quality_label, output_path, created_at, audio_format, audio_quality, group_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                    group_id,
                ],
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
            ids.push(tx.last_insert_rowid() as u64);
        }

        tx.commit()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok((group_id, ids))
    }

    /// Paginated queue view where each batch group collapses into a single top-level
    /// row. The pagination unit is the top-level row (a group header or a standalone
    /// item), so a group never straddles a page boundary regardless of its size.
    pub fn get_queue_grouped(
        &self,
        page: u32,
        page_size: u32,
        status_filter: Option<&str>,
    ) -> Result<QueueResult, AppError> {
        let page_size = page_size.clamp(1, 100);
        let offset = (page as u64) * (page_size as u64);

        // Collect global status counts, the top-level total, and this page's row
        // descriptors under one guard, then drop it before per-row detail lookups —
        // get_download() / header aggregation re-acquire the same non-reentrant Mutex.
        let (counts, total_count, rows): (StatusCounts, u64, Vec<(String, u64)>) = {
            let conn = self.conn();
            let counts = read_status_counts(&conn)?;

            let (total_count, rows) = if let Some(filter) = status_filter {
                let total: u64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM (
                            SELECT g.id FROM download_groups g
                              WHERE EXISTS (SELECT 1 FROM downloads d WHERE d.group_id = g.id AND d.status = ?1)
                            UNION ALL
                            SELECT d.id FROM downloads d WHERE d.group_id IS NULL AND d.status = ?1
                         )",
                        params![filter],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

                let mut stmt = conn
                    .prepare(
                        "SELECT row_kind, key_id FROM (
                            SELECT 'group' AS row_kind, g.id AS key_id, g.created_at AS sort_ts
                              FROM download_groups g
                              WHERE EXISTS (SELECT 1 FROM downloads d WHERE d.group_id = g.id AND d.status = ?1)
                            UNION ALL
                            SELECT 'single' AS row_kind, d.id AS key_id, d.created_at AS sort_ts
                              FROM downloads d WHERE d.group_id IS NULL AND d.status = ?1
                         ) ORDER BY sort_ts DESC LIMIT ?2 OFFSET ?3",
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                let rows = stmt
                    .query_map(params![filter, page_size, offset], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                    })
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                (total, rows)
            } else {
                let total: u64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM (
                            SELECT g.id FROM download_groups g
                              WHERE EXISTS (SELECT 1 FROM downloads d WHERE d.group_id = g.id)
                            UNION ALL
                            SELECT d.id FROM downloads d WHERE d.group_id IS NULL
                         )",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

                let mut stmt = conn
                    .prepare(
                        "SELECT row_kind, key_id FROM (
                            SELECT 'group' AS row_kind, g.id AS key_id, g.created_at AS sort_ts
                              FROM download_groups g
                              WHERE EXISTS (SELECT 1 FROM downloads d WHERE d.group_id = g.id)
                            UNION ALL
                            SELECT 'single' AS row_kind, d.id AS key_id, d.created_at AS sort_ts
                              FROM downloads d WHERE d.group_id IS NULL
                         ) ORDER BY sort_ts DESC LIMIT ?1 OFFSET ?2",
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                let rows = stmt
                    .query_map(params![page_size, offset], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                    })
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                (total, rows)
            };

            (counts, total_count, rows)
        }; // guard dropped here

        let mut entries = Vec::with_capacity(rows.len());
        for (row_kind, key_id) in rows {
            if row_kind == "group" {
                if let Some(group) = self.get_queue_group_header(key_id)? {
                    entries.push(QueueEntry::Group { group });
                }
            } else if let Some(item) = self.get_download(key_id)? {
                entries.push(QueueEntry::Single { item });
            }
        }

        Ok(QueueResult {
            items: entries,
            total_count,
            page,
            page_size,
            active_count: counts.active,
            pending_count: counts.pending,
            completed_count: counts.completed,
            failed_count: counts.failed,
            cancelled_count: counts.cancelled,
        })
    }

    /// Live aggregate for one queue group. Counts/progress reflect the rows still in
    /// the downloads table (clearing completed rows lowers them — accepted tradeoff),
    /// while total_count stays fixed from the group row.
    fn get_queue_group_header(&self, group_id: u64) -> Result<Option<QueueGroupHeader>, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT g.title, g.kind, g.total_count, g.created_at,
                COALESCE(SUM(CASE WHEN d.status = 'completed' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN d.status = 'failed' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN d.status = 'downloading' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN d.status = 'pending' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN d.status = 'completed' THEN 100.0
                                  WHEN d.status = 'downloading' THEN d.progress
                                  ELSE 0 END), 0.0)
             FROM download_groups g
             LEFT JOIN downloads d ON d.group_id = g.id
             WHERE g.id = ?1
             GROUP BY g.id",
            [group_id],
            |row| {
                let total_count: u64 = row.get(2)?;
                let progress_sum: f64 = row.get(8)?;
                Ok(QueueGroupHeader {
                    group_id,
                    title: row.get(0)?,
                    kind: row.get(1)?,
                    total_count,
                    created_at: row.get(3)?,
                    completed_count: row.get(4)?,
                    failed_count: row.get(5)?,
                    active_count: row.get(6)?,
                    pending_count: row.get(7)?,
                    progress: (progress_sum / total_count.max(1) as f64) as f32,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::DatabaseError(e.to_string()))
    }

    /// Paginated history view, grouped like the queue. History only ever holds
    /// completed items, so a group here is "the completed items of that batch".
    pub fn get_history_grouped(
        &self,
        page: u32,
        page_size: u32,
        search: Option<&str>,
    ) -> Result<HistoryResult, AppError> {
        let page_size = page_size.clamp(1, 100);
        let offset = (page as u64) * (page_size as u64);

        let (total_count, rows): (u64, Vec<(String, u64)>) = {
            let conn = self.conn();

            if let Some(s) = search {
                let escaped = s
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                let like = format!("%{}%", escaped);

                let total: u64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM (
                            SELECT h.group_id FROM history h
                              JOIN download_groups g ON g.id = h.group_id
                              WHERE h.group_id IS NOT NULL
                              GROUP BY h.group_id
                              HAVING g.title LIKE ?1 ESCAPE '\\'
                                  OR SUM(CASE WHEN h.title LIKE ?1 ESCAPE '\\' THEN 1 ELSE 0 END) > 0
                            UNION ALL
                            SELECT h.id FROM history h
                              WHERE h.group_id IS NULL AND h.title LIKE ?1 ESCAPE '\\'
                         )",
                        params![like],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

                let mut stmt = conn
                    .prepare(
                        "SELECT row_kind, key_id FROM (
                            SELECT 'group' AS row_kind, h.group_id AS key_id, MAX(h.downloaded_at) AS sort_ts
                              FROM history h
                              JOIN download_groups g ON g.id = h.group_id
                              WHERE h.group_id IS NOT NULL
                              GROUP BY h.group_id
                              HAVING g.title LIKE ?1 ESCAPE '\\'
                                  OR SUM(CASE WHEN h.title LIKE ?1 ESCAPE '\\' THEN 1 ELSE 0 END) > 0
                            UNION ALL
                            SELECT 'single' AS row_kind, h.id AS key_id, h.downloaded_at AS sort_ts
                              FROM history h
                              WHERE h.group_id IS NULL AND h.title LIKE ?1 ESCAPE '\\'
                         ) ORDER BY sort_ts DESC LIMIT ?2 OFFSET ?3",
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                let rows = stmt
                    .query_map(params![like, page_size, offset], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                    })
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                (total, rows)
            } else {
                let total: u64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM (
                            SELECT h.group_id FROM history h WHERE h.group_id IS NOT NULL GROUP BY h.group_id
                            UNION ALL
                            SELECT h.id FROM history h WHERE h.group_id IS NULL
                         )",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

                let mut stmt = conn
                    .prepare(
                        "SELECT row_kind, key_id FROM (
                            SELECT 'group' AS row_kind, h.group_id AS key_id, MAX(h.downloaded_at) AS sort_ts
                              FROM history h WHERE h.group_id IS NOT NULL GROUP BY h.group_id
                            UNION ALL
                            SELECT 'single' AS row_kind, h.id AS key_id, h.downloaded_at AS sort_ts
                              FROM history h WHERE h.group_id IS NULL
                         ) ORDER BY sort_ts DESC LIMIT ?1 OFFSET ?2",
                    )
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                let rows = stmt
                    .query_map(params![page_size, offset], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
                    })
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
                (total, rows)
            }
        }; // guard dropped here

        let mut entries = Vec::with_capacity(rows.len());
        for (row_kind, key_id) in rows {
            if row_kind == "group" {
                if let Some(group) = self.get_history_group_header(key_id)? {
                    entries.push(HistoryEntry::Group { group });
                }
            } else if let Some(item) = self.get_history_item(key_id)? {
                entries.push(HistoryEntry::Single { item });
            }
        }

        Ok(HistoryResult {
            items: entries,
            total_count,
            page,
            page_size,
        })
    }

    fn get_history_group_header(
        &self,
        group_id: u64,
    ) -> Result<Option<HistoryGroupHeader>, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT g.title, g.total_count, COUNT(h.id), COALESCE(MAX(h.downloaded_at), 0)
             FROM download_groups g
             JOIN history h ON h.group_id = g.id
             WHERE g.id = ?1
             GROUP BY g.id",
            [group_id],
            |row| {
                Ok(HistoryGroupHeader {
                    group_id,
                    title: row.get(0)?,
                    total_count: row.get(1)?,
                    completed_count: row.get(2)?,
                    latest_downloaded_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|e| AppError::DatabaseError(e.to_string()))
    }

    fn get_history_item(&self, id: u64) -> Result<Option<HistoryItem>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM history WHERE id = ?1",
                HISTORY_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        stmt.query_row([id], super::history::map_history_row)
            .optional()
            .map_err(|e| AppError::DatabaseError(e.to_string()))
    }

    /// Items of a queue group, loaded lazily when the group is expanded in the UI.
    pub fn get_group_queue_items(&self, group_id: u64) -> Result<Vec<DownloadTaskInfo>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM downloads WHERE group_id = ?1 ORDER BY created_at ASC",
                super::queue::DOWNLOAD_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        let items = stmt
            .query_map([group_id], super::queue::map_download_row)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(items)
    }

    /// Completed items of a history group, loaded lazily on expand.
    pub fn get_group_history_items(&self, group_id: u64) -> Result<Vec<HistoryItem>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM history WHERE group_id = ?1 ORDER BY downloaded_at ASC",
                HISTORY_COLUMNS
            ))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        let items = stmt
            .query_map([group_id], super::history::map_history_row)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(items)
    }

    /// Cancellable (pending/downloading) ids within a group — used by cancel_group.
    pub fn get_cancellable_ids_in_group(&self, group_id: u64) -> Result<Vec<u64>, AppError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id FROM downloads WHERE group_id = ?1 AND status IN ('downloading', 'pending')",
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        let ids = stmt
            .query_map([group_id], |row| row.get(0))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<u64>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(ids)
    }

    pub fn delete_group_history(&self, group_id: u64) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute("DELETE FROM history WHERE group_id = ?1", params![group_id])
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(())
    }
}
