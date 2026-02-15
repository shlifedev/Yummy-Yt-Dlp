use crate::modules::types::AppError;
use crate::ytdlp::types::{LogEntry, LogQueryResult, LogStats};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

pub struct LogDatabase {
    conn: Mutex<Connection>,
}

impl LogDatabase {
    pub fn new(app_data_dir: &Path) -> Result<Self, AppError> {
        std::fs::create_dir_all(app_data_dir).map_err(|e| {
            AppError::DatabaseError(format!("Failed to create app data dir: {}", e))
        })?;

        let db_path = app_data_dir.join("logs.db");
        let conn =
            Connection::open(&db_path).map_err(|e| AppError::DatabaseError(e.to_string()))?;

        // WAL mode + relaxed sync for write performance
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;",
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Self::create_tables(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn create_tables(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                level TEXT NOT NULL,
                category TEXT NOT NULL,
                message TEXT NOT NULL,
                details TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);
            CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);
            CREATE INDEX IF NOT EXISTS idx_logs_category ON logs(category);",
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub fn insert_log(
        &self,
        timestamp: i64,
        level: &str,
        category: &str,
        message: &str,
        details: Option<&str>,
    ) -> Result<i64, AppError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO logs (timestamp, level, category, message, details) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![timestamp, level, category, message, details],
        )
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(conn.last_insert_rowid())
    }

    pub fn query_logs(
        &self,
        page: u32,
        page_size: u32,
        level: Option<&str>,
        category: Option<&str>,
        search: Option<&str>,
        since: Option<i64>,
    ) -> Result<LogQueryResult, AppError> {
        let page_size = page_size.clamp(1, 200);
        let conn = self.conn();

        let mut conditions: Vec<String> = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut param_idx = 1u32;

        if let Some(l) = level {
            conditions.push(format!("level = ?{}", param_idx));
            param_values.push(Box::new(l.to_string()));
            param_idx += 1;
        }

        if let Some(c) = category {
            conditions.push(format!("category = ?{}", param_idx));
            param_values.push(Box::new(c.to_string()));
            param_idx += 1;
        }

        if let Some(s) = search {
            let escaped = s
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            conditions.push(format!("message LIKE ?{} ESCAPE '\\'", param_idx));
            param_values.push(Box::new(format!("%{}%", escaped)));
            param_idx += 1;
        }

        if let Some(ts) = since {
            conditions.push(format!("timestamp > ?{}", param_idx));
            param_values.push(Box::new(ts));
            param_idx += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        // Count query
        let total_count: u64;
        let count_sql = format!("SELECT COUNT(*) FROM logs {}", where_clause);
        {
            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p.as_ref()).collect();
            total_count = conn
                .query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        // Data query
        let offset = page * page_size;
        let data_sql = format!(
            "SELECT id, timestamp, level, category, message, details FROM logs {} ORDER BY timestamp DESC, id DESC LIMIT ?{} OFFSET ?{}",
            where_clause, param_idx, param_idx + 1
        );

        param_values.push(Box::new(page_size));
        param_values.push(Box::new(offset));

        let data_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&data_sql)
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let items = stmt
            .query_map(data_refs.as_slice(), |row| {
                Ok(LogEntry {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    level: row.get(2)?,
                    category: row.get(3)?,
                    message: row.get(4)?,
                    details: row.get(5)?,
                })
            })
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(LogQueryResult {
            items,
            total_count,
            page,
            page_size,
        })
    }

    pub fn get_log_stats(&self) -> Result<LogStats, AppError> {
        let conn = self.conn();

        let total_count: u64 = conn
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let error_count: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM logs WHERE level = 'ERROR'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let warn_count: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM logs WHERE level = 'WARN'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let info_count: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM logs WHERE level = 'INFO'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(LogStats {
            total_count,
            error_count,
            warn_count,
            info_count,
        })
    }

    pub fn clear_logs(&self, before_timestamp: Option<i64>) -> Result<u64, AppError> {
        let conn = self.conn();

        let deleted = if let Some(ts) = before_timestamp {
            conn.execute("DELETE FROM logs WHERE timestamp <= ?1", params![ts])
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
        } else {
            conn.execute("DELETE FROM logs", [])
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
        };

        Ok(deleted as u64)
    }

    /// Delete all log data (used by factory reset).
    /// Uses the live connection instead of deleting the DB file.
    pub fn clear_all_data(&self) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute("DELETE FROM logs", [])
            .map_err(|e| AppError::DatabaseError(format!("Failed to clear logs: {}", e)))?;
        conn.execute_batch("VACUUM;")
            .map_err(|e| AppError::DatabaseError(format!("Failed to vacuum logs: {}", e)))?;
        Ok(())
    }

    pub fn cleanup_old_logs(&self, max_age_days: u32, max_entries: u64) -> Result<u64, AppError> {
        let conn = self.conn();
        let mut total_deleted = 0u64;

        // Delete by age
        let cutoff =
            chrono::Utc::now().timestamp_millis() - (max_age_days as i64 * 24 * 60 * 60 * 1000);
        let deleted = conn
            .execute("DELETE FROM logs WHERE timestamp < ?1", params![cutoff])
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        total_deleted += deleted as u64;

        // Delete excess entries (keep newest max_entries)
        let count: u64 = conn
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        if count > max_entries {
            let deleted = conn
                .execute(
                    "DELETE FROM logs WHERE id NOT IN (SELECT id FROM logs ORDER BY timestamp DESC, id DESC LIMIT ?1)",
                    params![max_entries],
                )
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;
            total_deleted += deleted as u64;
        }

        Ok(total_deleted)
    }
}
