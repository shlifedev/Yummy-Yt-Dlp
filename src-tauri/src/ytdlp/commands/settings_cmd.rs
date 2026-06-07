use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::binary;
use crate::ytdlp::download::DownloadManager;
use crate::ytdlp::security;
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
    // Validate settings before saving
    if !settings.download_path.is_empty() {
        security::sanitize_output_path(&settings.download_path)?;
    }
    security::sanitize_filename_template(&settings.filename_template)?;
    if let Some(ref browser) = settings.cookie_browser {
        security::sanitize_cookie_browser(browser)?;
    }

    // Validate advanced free-text options (empty = unset, allowed). Enum/allowlist fields are
    // additionally re-checked at download time in download::advanced::build_advanced_args.
    let adv = &settings.advanced;
    if !adv.sub_langs.is_empty() {
        security::sanitize_sub_langs(&adv.sub_langs)?;
    }
    if !adv.limit_rate.is_empty() {
        security::sanitize_limit_rate(&adv.limit_rate)?;
    }
    if !adv.download_sections.is_empty() {
        security::sanitize_download_sections(&adv.download_sections)?;
    }
    if !adv.proxy.is_empty() {
        security::sanitize_proxy(&adv.proxy)?;
    }

    // Clamp max_concurrent to safe range
    let mut settings = settings;
    settings.max_concurrent = security::clamp_max_concurrent(settings.max_concurrent);

    // Snapshot the dependency-affecting settings to know whether to drop the cache.
    let old = crate::ytdlp::settings::get_settings(&app).ok();

    crate::ytdlp::settings::update_settings(&app, &settings)?;

    // Sync max_concurrent to DownloadManager at runtime
    let manager = app.state::<Arc<DownloadManager>>();
    manager.set_max_concurrent(settings.max_concurrent);

    // Invalidate dep cache when the mode or any per-item override changes.
    let dep_changed = old
        .map(|s| s.dep_mode != settings.dep_mode || s.dep_overrides != settings.dep_overrides)
        .unwrap_or(true);
    if dep_changed {
        binary::invalidate_dep_cache();
    }

    logger::info_cat("settings", "Settings updated");

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn select_download_directory(app: AppHandle) -> Result<Option<String>, AppError> {
    let title = folder_dialog_title(&app);
    // Use spawn_blocking to avoid blocking the async runtime
    let result = tokio::task::spawn_blocking(move || {
        app.dialog().file().set_title(title).blocking_pick_folder()
    })
    .await
    .map_err(|e| AppError::Custom(format!("Dialog task failed: {}", e)))?;

    Ok(result.map(|p| p.to_string()))
}

/// Localized folder-picker title based on the saved language. The OS dialog is invoked from
/// Rust and can't use the frontend i18n, so this mirrors the tray.rs label approach.
fn folder_dialog_title(app: &AppHandle) -> &'static str {
    let lang = crate::ytdlp::settings::get_settings(app)
        .ok()
        .and_then(|s| s.language)
        .unwrap_or_default();
    match lang.as_str() {
        "ko" => "다운로드 폴더 선택",
        "ja" => "ダウンロードフォルダを選択",
        "zh-CN" => "选择下载文件夹",
        "zh-TW" => "選擇下載資料夾",
        "fr" => "Choisir le dossier de téléchargement",
        "de" => "Download-Ordner auswählen",
        _ => "Select Download Folder",
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_available_browsers() -> Vec<String> {
    let mut browsers = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let checks: &[(&str, &str)] = &[
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
    }

    #[cfg(target_os = "macos")]
    {
        let checks: &[(&str, &str)] = &[
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
    }

    #[cfg(target_os = "linux")]
    {
        let checks: &[(&str, &str)] = &[
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
