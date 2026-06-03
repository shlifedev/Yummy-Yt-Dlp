use super::Database;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use rusqlite::{params, OptionalExtension};

const HISTORY_COLUMNS: &str =
    "id, video_url, video_id, title, quality_label, format, file_path, file_size, downloaded_at";

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

    /// Paginated history view, grouped by batch. Each batch group collapses into a single
    /// top-level row (a header), standalone completed items stay as their own rows. The
    /// pagination unit is the top-level row, so a group never straddles a page boundary.
    /// History only ever holds completed items, so a group here is "the completed items
    /// of that batch".
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

    /// Completed items of a history group, loaded lazily when the group is expanded.
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
