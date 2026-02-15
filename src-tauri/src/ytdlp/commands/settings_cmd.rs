use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::binary;
use crate::ytdlp::download::DownloadManager;
use crate::ytdlp::types::*;
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
#[specta::specta]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, AppError> {
    crate::ytdlp::settings::get_settings(&app)
}

#[tauri::command]
#[specta::specta]
pub fn update_settings(app: AppHandle, settings: AppSettings) -> Result<(), AppError> {
    // Check if dep_mode changed to invalidate cache
    let old_dep_mode = crate::ytdlp::settings::get_settings(&app)
        .map(|s| s.dep_mode)
        .unwrap_or_default();

    crate::ytdlp::settings::update_settings(&app, &settings)?;

    // Sync max_concurrent to DownloadManager at runtime
    let manager = app.state::<Arc<DownloadManager>>();
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
    // Use spawn_blocking to avoid blocking the async runtime
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
        // Linux - check common installation paths
        let checks = vec![
            ("chrome", "/usr/bin/google-chrome-stable"),
            ("chrome", "/usr/bin/google-chrome"),
            ("chromium", "/usr/bin/chromium-browser"),
            ("chromium", "/usr/bin/chromium"),
            ("firefox", "/usr/bin/firefox"),
            ("brave", "/usr/bin/brave-browser"),
            ("edge", "/usr/bin/microsoft-edge"),
        ];

        for (name, path) in checks {
            if std::path::Path::new(path).exists() && !browsers.contains(&name.to_string()) {
                browsers.push(name.to_string());
            }
        }
    }

    browsers
}
