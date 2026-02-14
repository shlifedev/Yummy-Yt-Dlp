use super::binary;
use super::types::*;
use crate::modules::types::AppError;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
#[specta::specta]
pub async fn check_dependencies() -> Result<DependencyStatus, AppError> {
    Ok(binary::check_dependencies().await)
}

#[tauri::command]
#[specta::specta]
pub async fn update_ytdlp() -> Result<String, AppError> {
    binary::update_ytdlp().await
}

#[tauri::command]
#[specta::specta]
pub async fn get_download_queue(app: AppHandle) -> Result<Vec<DownloadTaskInfo>, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_download_queue()
}

#[tauri::command]
#[specta::specta]
pub async fn clear_completed(app: AppHandle) -> Result<u32, AppError> {
    let db = app.state::<crate::DbState>();
    db.clear_completed()
}

#[tauri::command]
#[specta::specta]
pub async fn retry_download(
    app: AppHandle,
    task_id: u64,
    on_event: Channel<DownloadEvent>,
) -> Result<(), AppError> {
    let _ = on_event; // Suppress unused warning (legacy parameter)

    // Get the original download info from DB
    let db = app.state::<crate::DbState>();
    let _task = db
        .get_download(task_id)?
        .ok_or_else(|| AppError::Custom("Download task not found".to_string()))?;

    // Reset the original task to pending (reuse existing DB row instead of
    // creating a duplicate via add_to_queue, which would leave a zombie pending row)
    db.update_download_status(task_id, &DownloadStatus::Pending, None)?;

    // Try to acquire a slot and start the download immediately if possible
    let manager = app.state::<Arc<super::download::DownloadManager>>();
    if manager.try_acquire() {
        db.update_download_status(task_id, &DownloadStatus::Downloading, None)?;
        let app_clone = app.clone();
        let app_panic_guard = app.clone();
        tokio::spawn(async move {
            let result = tokio::spawn(async move {
                super::download::execute_download_public(app_clone, task_id).await;
            })
            .await;
            if let Err(e) = result {
                eprintln!("Download task panicked: {:?}", e);
                let manager = app_panic_guard.state::<Arc<super::download::DownloadManager>>();
                manager.release();
                super::download::process_next_pending_public(app_panic_guard);
            }
        });
    }
    // Otherwise stays pending, will be picked up by process_next_pending when a slot frees

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, AppError> {
    super::settings::get_settings(&app)
}

#[tauri::command]
#[specta::specta]
pub fn update_settings(app: AppHandle, settings: AppSettings) -> Result<(), AppError> {
    super::settings::update_settings(&app, &settings)?;

    // 2-1: Sync max_concurrent to DownloadManager at runtime
    let manager = app.state::<Arc<super::download::DownloadManager>>();
    manager.set_max_concurrent(settings.max_concurrent);

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn select_download_directory(app: AppHandle) -> Result<Option<String>, AppError> {
    // 2-4: Use spawn_blocking to avoid blocking the async runtime
    let result = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("다운로드 폴더 선택")
            .blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::Custom(format!("Dialog task failed: {}", e)))?;

    Ok(result.map(|p| p.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn get_available_browsers() -> Vec<String> {
    let mut browsers = Vec::new();

    if cfg!(target_os = "windows") {
        // Check common browser paths on Windows
        let checks = vec![
            (
                "chrome",
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            ),
            (
                "chrome",
                r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            ),
            ("firefox", r"C:\Program Files\Mozilla Firefox\firefox.exe"),
            (
                "firefox",
                r"C:\Program Files (x86)\Mozilla Firefox\firefox.exe",
            ),
            (
                "edge",
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            ),
            (
                "brave",
                r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
            ),
        ];

        for (name, path) in checks {
            if std::path::Path::new(path).exists() && !browsers.contains(&name.to_string()) {
                browsers.push(name.to_string());
            }
        }
    } else if cfg!(target_os = "macos") {
        let checks = vec![
            ("chrome", "/Applications/Google Chrome.app"),
            ("firefox", "/Applications/Firefox.app"),
            ("safari", "/Applications/Safari.app"),
            ("brave", "/Applications/Brave Browser.app"),
            ("edge", "/Applications/Microsoft Edge.app"),
        ];

        for (name, path) in checks {
            if std::path::Path::new(path).exists() {
                browsers.push(name.to_string());
            }
        }
    } else {
        // Linux - check if commands exist using which
        for name in &["chrome", "chromium", "firefox", "brave"] {
            browsers.push(name.to_string());
        }
    }

    browsers
}

#[tauri::command]
#[specta::specta]
pub async fn get_download_history(
    app: AppHandle,
    page: u32,
    page_size: u32,
    search: Option<String>,
) -> Result<HistoryResult, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_history(page, page_size, search.as_deref())
}

#[tauri::command]
#[specta::specta]
pub async fn check_duplicate(
    app: AppHandle,
    video_id: String,
) -> Result<DuplicateCheckResult, AppError> {
    let db = app.state::<crate::DbState>();
    let history_item = db.check_duplicate(&video_id)?;
    let in_queue = db.check_duplicate_in_queue(&video_id)?;
    Ok(DuplicateCheckResult {
        in_history: history_item.is_some(),
        in_queue,
        history_item,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn delete_history_item(app: AppHandle, id: u64) -> Result<(), AppError> {
    let db = app.state::<crate::DbState>();
    db.delete_history(id)
}

#[tauri::command]
#[specta::specta]
pub async fn get_active_downloads(app: AppHandle) -> Result<Vec<DownloadTaskInfo>, AppError> {
    let db = app.state::<crate::DbState>();
    db.get_active_downloads()
}

#[tauri::command]
#[specta::specta]
pub fn set_minimize_to_tray(
    app: AppHandle,
    minimize: bool,
    remember: bool,
) -> Result<(), AppError> {
    if remember {
        super::tray::set_minimize_to_tray_setting(&app, minimize)?;
    }

    if minimize {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
        }
    } else {
        let manager = app.state::<Arc<super::download::DownloadManager>>();
        manager.cancel_all();
        app.exit(0);
    }

    Ok(())
}
