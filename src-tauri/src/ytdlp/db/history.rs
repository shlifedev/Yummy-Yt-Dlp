use super::Database;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use rusqlite::params;

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

    pub fn get_history(
        &self,
        page: u32,
        page_size: u32,
        search: Option<&str>,
    ) -> Result<HistoryResult, AppError> {
        let page_size = page_size.clamp(1, 100);
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
}
