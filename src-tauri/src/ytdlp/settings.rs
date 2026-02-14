use super::types::AppSettings;
use crate::modules::types::AppError;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";

pub fn get_settings(app: &AppHandle) -> Result<AppSettings, AppError> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Custom(e.to_string()))?;

    let defaults = AppSettings::default();

    let download_path = store
        .get("downloadPath")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| {
            let path = default_download_path();
            if path.is_empty() {
                defaults.download_path
            } else {
                path
            }
        });

    let default_quality = store
        .get("defaultQuality")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or(defaults.default_quality);

    let max_concurrent = store
        .get("maxConcurrent")
        .and_then(|v| v.as_u64().map(|n| n as u32))
        .unwrap_or(defaults.max_concurrent);

    let filename_template = store
        .get("filenameTemplate")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or(defaults.filename_template);

    let cookie_browser = store
        .get("cookieBrowser")
        .and_then(|v| v.as_str().map(String::from));

    let auto_update_ytdlp = store
        .get("autoUpdateYtdlp")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.auto_update_ytdlp);

    let use_advanced_template = store
        .get("useAdvancedTemplate")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.use_advanced_template);

    let template_uploader_folder = store
        .get("templateUploaderFolder")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_uploader_folder);

    let template_upload_date = store
        .get("templateUploadDate")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_upload_date);

    let template_video_id = store
        .get("templateVideoId")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_video_id);

    let language = store
        .get("language")
        .and_then(|v| v.as_str().map(String::from));

    let theme = store
        .get("theme")
        .and_then(|v| v.as_str().map(String::from));

    let minimize_to_tray = store.get("minimizeToTray").and_then(|v| v.as_bool());

    Ok(AppSettings {
        download_path,
        default_quality,
        max_concurrent,
        filename_template,
        cookie_browser,
        auto_update_ytdlp,
        use_advanced_template,
        template_uploader_folder,
        template_upload_date,
        template_video_id,
        language,
        theme,
        minimize_to_tray,
    })
}

pub fn update_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), AppError> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Custom(e.to_string()))?;

    store.set(
        "downloadPath",
        serde_json::to_value(&settings.download_path)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "defaultQuality",
        serde_json::to_value(&settings.default_quality)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "maxConcurrent",
        serde_json::to_value(settings.max_concurrent)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "filenameTemplate",
        serde_json::to_value(&settings.filename_template)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "cookieBrowser",
        serde_json::to_value(&settings.cookie_browser)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "autoUpdateYtdlp",
        serde_json::to_value(settings.auto_update_ytdlp)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "useAdvancedTemplate",
        serde_json::to_value(settings.use_advanced_template)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "templateUploaderFolder",
        serde_json::to_value(settings.template_uploader_folder)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "templateUploadDate",
        serde_json::to_value(settings.template_upload_date)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "templateVideoId",
        serde_json::to_value(settings.template_video_id)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "language",
        serde_json::to_value(&settings.language).map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "theme",
        serde_json::to_value(&settings.theme).map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "minimizeToTray",
        serde_json::to_value(settings.minimize_to_tray)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.save().map_err(|e| AppError::Custom(e.to_string()))?;

    Ok(())
}

pub fn default_download_path() -> String {
    if cfg!(target_os = "windows") {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return format!(r"{}\Downloads", profile);
        }
    } else if let Ok(home) = std::env::var("HOME") {
        return format!("{}/Downloads", home);
    }

    String::from(".")
}

pub fn get_settings_from_path(app_data_dir: &std::path::Path) -> Result<AppSettings, AppError> {
    let settings_path = app_data_dir.join("settings.json");

    if !settings_path.exists() {
        return Ok(AppSettings::default());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| AppError::Custom(format!("Failed to read settings file: {}", e)))?;

    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Custom(format!("Failed to parse settings: {}", e)))?;

    let defaults = AppSettings::default();

    let download_path = value
        .get("downloadPath")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| {
            let path = default_download_path();
            if path.is_empty() {
                defaults.download_path.clone()
            } else {
                path
            }
        });

    let default_quality = value
        .get("defaultQuality")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or(defaults.default_quality);

    let max_concurrent = value
        .get("maxConcurrent")
        .and_then(|v| v.as_u64().map(|n| n as u32))
        .unwrap_or(defaults.max_concurrent);

    let filename_template = value
        .get("filenameTemplate")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or(defaults.filename_template);

    let cookie_browser = value
        .get("cookieBrowser")
        .and_then(|v| v.as_str().map(String::from));

    let auto_update_ytdlp = value
        .get("autoUpdateYtdlp")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.auto_update_ytdlp);

    let use_advanced_template = value
        .get("useAdvancedTemplate")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.use_advanced_template);

    let template_uploader_folder = value
        .get("templateUploaderFolder")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_uploader_folder);

    let template_upload_date = value
        .get("templateUploadDate")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_upload_date);

    let template_video_id = value
        .get("templateVideoId")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_video_id);

    let language = value
        .get("language")
        .and_then(|v| v.as_str().map(String::from));

    let theme = value
        .get("theme")
        .and_then(|v| v.as_str().map(String::from));

    let minimize_to_tray = value.get("minimizeToTray").and_then(|v| v.as_bool());

    Ok(AppSettings {
        download_path,
        default_quality,
        max_concurrent,
        filename_template,
        cookie_browser,
        auto_update_ytdlp,
        use_advanced_template,
        template_uploader_folder,
        template_upload_date,
        template_video_id,
        language,
        theme,
        minimize_to_tray,
    })
}
