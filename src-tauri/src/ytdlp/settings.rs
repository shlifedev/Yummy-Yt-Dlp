use super::types::{AdvancedOptions, AppSettings};
use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::security;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
/// Last known-good copy of settings.json, refreshed after each successful save.
const BACKUP_FILE: &str = "settings.json.bak";

/// Common parsing logic: extract AppSettings from a key-value getter function.
/// `getter` takes a key name and returns an optional serde_json::Value.
fn parse_settings(getter: impl Fn(&str) -> Option<serde_json::Value>) -> AppSettings {
    let defaults = AppSettings::default();

    let download_path = getter("downloadPath")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| {
            let path = default_download_path();
            if path.is_empty() {
                defaults.download_path.clone()
            } else {
                path
            }
        });

    let default_quality = getter("defaultQuality")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or(defaults.default_quality);

    let max_concurrent = getter("maxConcurrent")
        .and_then(|v| v.as_u64().map(|n| security::clamp_max_concurrent(n as u32)))
        .unwrap_or(defaults.max_concurrent);

    let filename_template = getter("filenameTemplate")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or(defaults.filename_template);

    let cookie_browser = getter("cookieBrowser").and_then(|v| v.as_str().map(String::from));

    let auto_update_ytdlp = getter("autoUpdateYtdlp")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.auto_update_ytdlp);

    let use_advanced_template = getter("useAdvancedTemplate")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.use_advanced_template);

    let template_uploader_folder = getter("templateUploaderFolder")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_uploader_folder);

    let template_upload_date = getter("templateUploadDate")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_upload_date);

    let template_video_id = getter("templateVideoId")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.template_video_id);

    let language = getter("language").and_then(|v| v.as_str().map(String::from));

    let theme = getter("theme").and_then(|v| v.as_str().map(String::from));

    let minimize_to_tray = getter("minimizeToTray").and_then(|v| v.as_bool());

    let dep_mode = getter("depMode")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| defaults.dep_mode.clone());

    let dep_overrides = getter("depOverrides")
        .and_then(|v| serde_json::from_value::<std::collections::HashMap<String, String>>(v).ok())
        .unwrap_or_else(|| defaults.dep_overrides.clone());

    let setup_completed = getter("setupCompleted")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.setup_completed);

    let advanced = getter("advanced")
        .and_then(|v| serde_json::from_value::<AdvancedOptions>(v).ok())
        .unwrap_or_default();

    AppSettings {
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
        dep_mode,
        dep_overrides,
        advanced,
        setup_completed,
    }
}

pub fn get_settings(app: &AppHandle) -> Result<AppSettings, AppError> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Custom(e.to_string()))?;

    Ok(parse_settings(|key| store.get(key)))
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
        serde_json::to_value(security::clamp_max_concurrent(settings.max_concurrent))
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

    store.set(
        "depMode",
        serde_json::to_value(&settings.dep_mode).map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "depOverrides",
        serde_json::to_value(&settings.dep_overrides)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "advanced",
        serde_json::to_value(&settings.advanced).map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.set(
        "setupCompleted",
        serde_json::to_value(settings.setup_completed)
            .map_err(|e| AppError::Custom(e.to_string()))?,
    );

    store.save().map_err(|e| AppError::Custom(e.to_string()))?;

    // Refresh the last known-good backup now that a full snapshot was saved successfully.
    if let Ok(app_data_dir) = app.path().app_data_dir() {
        backup_settings_file(&app_data_dir);
    }

    Ok(())
}

/// True when the file exists, is readable, and parses as JSON.
fn parses_as_json(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .map(|content| serde_json::from_str::<serde_json::Value>(&content).is_ok())
        .unwrap_or(false)
}

/// Recover from a settings.json corrupted by a crash or power loss mid-save.
///
/// Must run BEFORE the first `app.store()` call: tauri-plugin-store ignores load errors, so a
/// corrupt file silently yields an empty store and the next save would permanently overwrite
/// every setting with defaults. The corrupt file is quarantined (kept for diagnosis) and the
/// last known-good backup is restored, but only when the backup itself parses.
pub fn recover_corrupt_settings(app_data_dir: &std::path::Path) {
    let settings_path = app_data_dir.join(STORE_FILE);
    if !settings_path.exists() || parses_as_json(&settings_path) {
        return;
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let quarantine = app_data_dir.join(format!("{}.corrupt-{}", STORE_FILE, ts));
    if let Err(e) = std::fs::rename(&settings_path, &quarantine) {
        logger::warn_cat(
            "settings",
            &format!(
                "settings.json is corrupt but could not be quarantined: {}",
                e
            ),
        );
        return;
    }
    logger::warn_cat(
        "settings",
        &format!(
            "settings.json was corrupt; quarantined as {}",
            quarantine.display()
        ),
    );

    let backup = app_data_dir.join(BACKUP_FILE);
    if parses_as_json(&backup) {
        match std::fs::copy(&backup, &settings_path) {
            Ok(_) => logger::warn_cat("settings", "Restored settings.json from settings.json.bak"),
            Err(e) => logger::warn_cat(
                "settings",
                &format!("Failed to restore settings.json from backup: {}", e),
            ),
        }
    } else {
        logger::warn_cat(
            "settings",
            "No valid settings.json.bak to restore; starting from defaults",
        );
    }
}

/// Refresh the last known-good backup after a successful save: read the just-written file back
/// and copy it only when it parses, so settings.json.bak is always restorable. Store writes done
/// outside update_settings (e.g. the dep auto-update throttle timestamp) are not backed up;
/// losing those on a restore is harmless.
fn backup_settings_file(app_data_dir: &std::path::Path) {
    let settings_path = app_data_dir.join(STORE_FILE);
    if !parses_as_json(&settings_path) {
        logger::warn_cat(
            "settings",
            "Skipping settings backup: settings.json did not read back as valid JSON",
        );
        return;
    }
    if let Err(e) = std::fs::copy(&settings_path, app_data_dir.join(BACKUP_FILE)) {
        logger::warn_cat(
            "settings",
            &format!("Failed to write settings.json.bak: {}", e),
        );
    }
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

    Ok(parse_settings(|key| value.get(key).cloned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "yummy-settings-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn recover_quarantines_corrupt_file_and_restores_backup() {
        let dir = temp_dir("restore");
        std::fs::write(dir.join(STORE_FILE), "{ truncated").unwrap();
        std::fs::write(dir.join(BACKUP_FILE), r#"{"downloadPath":"/tmp"}"#).unwrap();

        recover_corrupt_settings(&dir);

        assert_eq!(
            std::fs::read_to_string(dir.join(STORE_FILE)).unwrap(),
            r#"{"downloadPath":"/tmp"}"#
        );
        let quarantined = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("settings.json.corrupt-")
            });
        assert!(quarantined);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_leaves_valid_settings_alone() {
        let dir = temp_dir("valid");
        std::fs::write(dir.join(STORE_FILE), r#"{"downloadPath":"/tmp"}"#).unwrap();

        recover_corrupt_settings(&dir);

        assert_eq!(
            std::fs::read_to_string(dir.join(STORE_FILE)).unwrap(),
            r#"{"downloadPath":"/tmp"}"#
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_does_not_restore_corrupt_backup() {
        let dir = temp_dir("badbak");
        std::fs::write(dir.join(STORE_FILE), "{ truncated").unwrap();
        std::fs::write(dir.join(BACKUP_FILE), "also broken").unwrap();

        recover_corrupt_settings(&dir);

        // The corrupt main file is quarantined and NOT replaced by the broken backup,
        // so the store starts from defaults instead of another corrupt file.
        assert!(!dir.join(STORE_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
