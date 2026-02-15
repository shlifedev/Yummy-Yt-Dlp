use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::binary;
use crate::ytdlp::download::DownloadManager;
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

#[tauri::command]
#[specta::specta]
pub fn set_minimize_to_tray(
    app: AppHandle,
    minimize: bool,
    remember: bool,
) -> Result<(), AppError> {
    if remember {
        crate::ytdlp::tray::set_minimize_to_tray_setting(&app, minimize)?;
    }

    if minimize {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    } else {
        let manager = app.state::<Arc<DownloadManager>>();
        manager.cancel_all();
        // Wait briefly for cancel signals to propagate and yt-dlp processes to terminate
        let manager_clone = manager.inner().clone();
        let app_clone = app.clone();
        tokio::spawn(async move {
            // Wait up to 3 seconds for active downloads to finish
            for _ in 0..30 {
                if manager_clone.active_count() == 0 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            app_clone.exit(0);
        });
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_recent_logs() -> String {
    logger::read_recent_logs(200)
}

/// Full factory reset: delete settings, app-managed binaries, databases, and caches.
#[tauri::command]
#[specta::specta]
pub async fn reset_all_data(app: AppHandle) -> Result<Vec<String>, AppError> {
    let mut results = Vec::new();
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Custom(format!("Failed to get app data dir: {}", e)))?;

    // 1. Clear settings store
    match app.store("settings.json") {
        Ok(store) => {
            store.clear();
            if let Err(e) = store.save() {
                results.push(format!("settings: clear failed - {}", e));
            } else {
                results.push("settings: cleared".to_string());
            }
        }
        Err(e) => results.push(format!("settings: error - {}", e)),
    }

    // 2. Delete app-managed binaries (bin/ directory)
    let bin_dir = app_data_dir.join("bin");
    if bin_dir.exists() {
        match tokio::fs::remove_dir_all(&bin_dir).await {
            Ok(_) => results.push("bin/: deleted".to_string()),
            Err(e) => results.push(format!("bin/: delete failed - {}", e)),
        }
    } else {
        results.push("bin/: not found (skip)".to_string());
    }

    // 3. Delete main database (ytdlp.db)
    let db_path = app_data_dir.join("ytdlp.db");
    if db_path.exists() {
        match tokio::fs::remove_file(&db_path).await {
            Ok(_) => results.push("ytdlp.db: deleted".to_string()),
            Err(e) => results.push(format!("ytdlp.db: delete failed - {}", e)),
        }
    }
    // Also delete WAL/SHM files if present
    for suffix in &["-wal", "-shm"] {
        let wal_path = app_data_dir.join(format!("ytdlp.db{}", suffix));
        if wal_path.exists() {
            let _ = tokio::fs::remove_file(&wal_path).await;
        }
    }

    // 4. Delete log database (logs.db)
    let log_db_path = app_data_dir.join("logs.db");
    if log_db_path.exists() {
        match tokio::fs::remove_file(&log_db_path).await {
            Ok(_) => results.push("logs.db: deleted".to_string()),
            Err(e) => results.push(format!("logs.db: delete failed - {}", e)),
        }
    }
    for suffix in &["-wal", "-shm"] {
        let wal_path = app_data_dir.join(format!("logs.db{}", suffix));
        if wal_path.exists() {
            let _ = tokio::fs::remove_file(&wal_path).await;
        }
    }

    // 5. Delete dep cache store
    let dep_cache_path = app_data_dir.join("dep-cache.json");
    if dep_cache_path.exists() {
        match tokio::fs::remove_file(&dep_cache_path).await {
            Ok(_) => results.push("dep-cache.json: deleted".to_string()),
            Err(e) => results.push(format!("dep-cache.json: delete failed - {}", e)),
        }
    }

    // 6. Invalidate in-memory caches
    binary::invalidate_dep_cache();
    results.push("memory cache: invalidated".to_string());

    logger::info_cat("debug", &format!("Full reset performed: {:?}", results));

    Ok(results)
}
