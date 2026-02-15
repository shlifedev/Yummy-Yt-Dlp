use super::binary;
use super::types::*;
use crate::modules::logger;
use crate::modules::types::AppError;
use std::sync::Arc;
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
pub async fn retry_download(app: AppHandle, task_id: u64) -> Result<(), AppError> {
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
    // Check if dep_mode changed to invalidate cache
    let old_dep_mode = super::settings::get_settings(&app)
        .map(|s| s.dep_mode)
        .unwrap_or_default();

    super::settings::update_settings(&app, &settings)?;

    // 2-1: Sync max_concurrent to DownloadManager at runtime
    let manager = app.state::<Arc<super::download::DownloadManager>>();
    manager.set_max_concurrent(settings.max_concurrent);

    // Invalidate dep cache when dep_mode changes
    if old_dep_mode != settings.dep_mode {
        binary::invalidate_dep_cache();
    }

    logger::info_cat("settings", "Settings updated");

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

#[tauri::command]
#[specta::specta]
pub fn get_cached_dep_status(
    app: AppHandle,
) -> Result<Option<super::types::FullDependencyStatus>, AppError> {
    Ok(binary::get_cached_dep_status(&app))
}

#[tauri::command]
#[specta::specta]
pub async fn check_full_dependencies(
    app: AppHandle,
    force: Option<bool>,
) -> Result<super::types::FullDependencyStatus, AppError> {
    if force.unwrap_or(false) {
        binary::invalidate_dep_cache();
    }
    let status = binary::check_full_dependencies(&app).await;
    logger::info_cat("dependency", &format!(
        "Dependency check: yt-dlp={}, ffmpeg={}, deno={}",
        status.ytdlp.installed, status.ffmpeg.installed, status.deno.installed
    ));
    Ok(status)
}

#[tauri::command]
#[specta::specta]
pub async fn install_dependency(app: AppHandle, dep_name: String) -> Result<String, AppError> {
    let result = match dep_name.as_str() {
        "yt-dlp" => super::dep_ytdlp::install_ytdlp(&app).await,
        "ffmpeg" => super::dep_ffmpeg::install_ffmpeg(&app).await,
        "deno" => super::dep_deno::install_deno(&app).await,
        _ => Err(AppError::DependencyInstallError(format!(
            "Unknown dependency: {}",
            dep_name
        ))),
    };
    binary::invalidate_dep_cache();
    result
}

#[tauri::command]
#[specta::specta]
pub async fn install_all_dependencies(app: AppHandle) -> Result<Vec<String>, AppError> {
    let status = binary::check_full_dependencies(&app).await;
    let mut results = Vec::new();

    // Run all three installations concurrently with tokio::join!
    let (ytdlp_res, ffmpeg_res, deno_res) = tokio::join!(
        async {
            if !status.ytdlp.installed {
                Some(super::dep_ytdlp::install_ytdlp(&app).await)
            } else {
                None
            }
        },
        async {
            if !status.ffmpeg.installed {
                Some(super::dep_ffmpeg::install_ffmpeg(&app).await)
            } else {
                None
            }
        },
        async {
            if !status.deno.installed {
                Some(super::dep_deno::install_deno(&app).await)
            } else {
                None
            }
        },
    );

    if let Some(res) = ytdlp_res {
        match res {
            Ok(v) => results.push(format!("yt-dlp: {}", v)),
            Err(e) => {
                super::dep_download::emit_stage(
                    &app,
                    "yt-dlp",
                    super::types::DepInstallStage::Failed,
                    Some(&e.to_string()),
                );
                results.push(format!("yt-dlp: FAILED - {}", e));
            }
        }
    }

    if let Some(res) = ffmpeg_res {
        match res {
            Ok(v) => results.push(format!("ffmpeg: {}", v)),
            Err(e) => {
                super::dep_download::emit_stage(
                    &app,
                    "ffmpeg",
                    super::types::DepInstallStage::Failed,
                    Some(&e.to_string()),
                );
                results.push(format!("ffmpeg: FAILED - {}", e));
            }
        }
    }

    if let Some(res) = deno_res {
        match res {
            Ok(v) => results.push(format!("deno: {}", v)),
            Err(e) => {
                super::dep_download::emit_stage(
                    &app,
                    "deno",
                    super::types::DepInstallStage::Failed,
                    Some(&e.to_string()),
                );
                results.push(format!("deno: FAILED - {}", e));
            }
        }
    }

    binary::invalidate_dep_cache();
    Ok(results)
}

#[tauri::command]
#[specta::specta]
pub async fn check_dependency_update(
    _app: AppHandle,
    dep_name: String,
) -> Result<super::types::DepUpdateInfo, AppError> {
    let latest = match dep_name.as_str() {
        "yt-dlp" => super::dep_ytdlp::get_latest_version().await?,
        "ffmpeg" => super::dep_ffmpeg::get_latest_version().await?,
        "deno" => super::dep_deno::get_latest_version().await?,
        _ => {
            return Err(AppError::DependencyInstallError(format!(
                "Unknown dependency: {}",
                dep_name
            )))
        }
    };

    Ok(super::types::DepUpdateInfo {
        current_version: None,
        latest_version: latest,
        update_available: true,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn update_dependency(app: AppHandle, dep_name: String) -> Result<String, AppError> {
    // Re-install (download latest) is the update mechanism
    install_dependency(app, dep_name).await
}

/// Delete an app-managed dependency binary from app_data_dir/bin/.
#[tauri::command]
#[specta::specta]
pub async fn delete_app_managed_dep(app: AppHandle, dep_name: String) -> Result<String, AppError> {
    let bin_dir = super::dep_download::ensure_bin_dir(&app)?;

    let names_to_delete: Vec<&str> = match dep_name.as_str() {
        "yt-dlp" => {
            if cfg!(target_os = "windows") {
                vec!["yt-dlp.exe"]
            } else {
                vec!["yt-dlp"]
            }
        }
        "ffmpeg" => {
            if cfg!(target_os = "windows") {
                vec!["ffmpeg.exe", "ffprobe.exe"]
            } else {
                vec!["ffmpeg", "ffprobe"]
            }
        }
        "deno" => {
            if cfg!(target_os = "windows") {
                vec!["deno.exe"]
            } else {
                vec!["deno"]
            }
        }
        _ => {
            return Err(AppError::DependencyInstallError(format!(
                "Unknown dependency: {}",
                dep_name
            )))
        }
    };

    let mut deleted = Vec::new();
    for name in &names_to_delete {
        let path = bin_dir.join(name);
        if path.exists() {
            tokio::fs::remove_file(&path).await.map_err(|e| {
                AppError::DependencyInstallError(format!("Failed to delete {}: {}", name, e))
            })?;
            deleted.push(name.to_string());
        }
    }

    binary::invalidate_dep_cache();
    if deleted.is_empty() {
        Ok(format!("{}: no app-managed binaries found", dep_name))
    } else {
        Ok(format!("{}: deleted {}", dep_name, deleted.join(", ")))
    }
}
