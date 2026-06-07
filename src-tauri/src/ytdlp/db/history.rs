use super::Database;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use rusqlite::params;

pub(super) fn map_history_row(row: &rusqlite::Row) -> rusqlite::Result<HistoryItem> {
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

impl Database {
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

        let result = stmt.query_row([video_id], map_history_row);

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
}
