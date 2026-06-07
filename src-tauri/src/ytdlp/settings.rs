use super::types::{AdvancedOptions, AppSettings};
use crate::modules::types::AppError;
use crate::ytdlp::security;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";

const VALID_DEP_MODES: &[&str] = &["hybrid", "bundled", "system", "external"];
const VALID_DEP_OVERRIDE_KEYS: &[&str] = &["yt-dlp", "ffmpeg", "deno"];
const VALID_DEP_OVERRIDE_VALUES: &[&str] = &["appManaged", "systemPath"];
const VALID_LANGUAGES: &[&str] = &["en", "ko", "ja", "de", "fr", "zh-CN", "zh-TW"];
const VALID_THEMES: &[&str] = &["dark", "red", "light"];
const VALID_SPONSORBLOCK_MODES: &[&str] = &["off", "mark", "remove"];
const VALID_SPONSORBLOCK_CATEGORIES: &[&str] = &[
    "sponsor",
    "intro",
    "outro",
    "selfpromo",
    "preview",
    "filler",
    "interaction",
    "music_offtopic",
];
const VALID_VIDEO_CODECS: &[&str] = &["auto", "av01", "vp9", "h264"];
const VALID_CONTAINER_FORMATS: &[&str] = &["", "mp4", "mkv", "webm"];
const VALID_SUB_CONVERT_FORMATS: &[&str] = &["", "srt", "ass", "vtt", "lrc"];

fn default_download_path_or_empty() -> String {
    let path = default_download_path();
    if path.is_empty() {
        AppSettings::default().download_path
    } else {
        path
    }
}

fn clean_cookie_browser(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let clean_chars = trimmed
        .chars()
        .all(|c| !c.is_control() && !matches!(c, '/' | '\\'));
    if clean_chars && !trimmed.contains("..") && security::sanitize_cookie_browser(trimmed).is_ok()
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn clean_choice(raw: Option<&str>, allowed: &[&str], default: Option<&str>) -> Option<String> {
    raw.and_then(|value| {
        let trimmed = value.trim();
        allowed
            .contains(&trimmed)
            .then(|| trimmed.to_string())
            .or_else(|| default.map(String::from))
    })
    .or_else(|| default.map(String::from))
}

fn clean_dep_overrides(
    raw: Option<std::collections::HashMap<String, String>>,
) -> std::collections::HashMap<String, String> {
    raw.unwrap_or_default()
        .into_iter()
        .filter(|(key, value)| {
            VALID_DEP_OVERRIDE_KEYS.contains(&key.as_str())
                && VALID_DEP_OVERRIDE_VALUES.contains(&value.as_str())
        })
        .collect()
}

fn clean_advanced_options(mut advanced: AdvancedOptions) -> AdvancedOptions {
    let defaults = AdvancedOptions::default();

    if security::sanitize_sub_langs(&advanced.sub_langs).is_err() {
        advanced.sub_langs = defaults.sub_langs.clone();
    }

    if !VALID_SUB_CONVERT_FORMATS.contains(&advanced.convert_subs.as_str()) {
        advanced.convert_subs = defaults.convert_subs.clone();
    }

    if !VALID_SPONSORBLOCK_MODES.contains(&advanced.sponsorblock_mode.as_str()) {
        advanced.sponsorblock_mode = defaults.sponsorblock_mode.clone();
    }

    if advanced
        .sponsorblock_categories
        .iter()
        .any(|category| !VALID_SPONSORBLOCK_CATEGORIES.contains(&category.as_str()))
    {
        advanced.sponsorblock_categories = defaults.sponsorblock_categories.clone();
    }

    if !VALID_VIDEO_CODECS.contains(&advanced.video_codec.as_str()) {
        advanced.video_codec = defaults.video_codec.clone();
    }

    if !advanced.limit_rate.trim().is_empty()
        && (advanced.limit_rate.len() > 32
            || security::sanitize_limit_rate(&advanced.limit_rate).is_err())
    {
        advanced.limit_rate = defaults.limit_rate.clone();
    }

    advanced.concurrent_fragments = advanced.concurrent_fragments.clamp(1, 16);

    if let Some(retries) = advanced.retries {
        advanced.retries = Some(retries.min(100));
    }

    advanced.sleep_interval = advanced.sleep_interval.min(86_400);

    if !VALID_CONTAINER_FORMATS.contains(&advanced.merge_output_format.as_str()) {
        advanced.merge_output_format = defaults.merge_output_format.clone();
    }

    if !VALID_CONTAINER_FORMATS.contains(&advanced.remux_video.as_str()) {
        advanced.remux_video = defaults.remux_video.clone();
    }

    if !advanced.download_sections.trim().is_empty()
        && security::sanitize_download_sections(&advanced.download_sections).is_err()
    {
        advanced.download_sections = defaults.download_sections.clone();
    }

    if !advanced.proxy.trim().is_empty() && security::sanitize_proxy(&advanced.proxy).is_err() {
        advanced.proxy = defaults.proxy.clone();
    }

    advanced
}

/// Common parsing logic: extract AppSettings from a key-value getter function.
/// `getter` takes a key name and returns an optional serde_json::Value.
fn parse_settings(getter: impl Fn(&str) -> Option<serde_json::Value>) -> AppSettings {
    let defaults = AppSettings::default();

    let download_path = getter("downloadPath")
        .and_then(|v| {
            v.as_str()
                .and_then(|path| security::sanitize_output_path(path).ok())
        })
        .unwrap_or_else(default_download_path_or_empty);

    let default_quality = getter("defaultQuality")
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or(defaults.default_quality);

    let max_concurrent = getter("maxConcurrent")
        .and_then(|v| {
            v.as_u64().map(|n| {
                let n = u32::try_from(n).unwrap_or(u32::MAX);
                security::clamp_max_concurrent(n)
            })
        })
        .unwrap_or(defaults.max_concurrent);

    let filename_template = getter("filenameTemplate")
        .and_then(|v| {
            v.as_str()
                .and_then(|template| security::sanitize_filename_template(template).ok())
        })
        .unwrap_or(defaults.filename_template);

    let cookie_browser =
        getter("cookieBrowser").and_then(|v| v.as_str().and_then(clean_cookie_browser));

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

    let language = clean_choice(
        getter("language").as_ref().and_then(|v| v.as_str()),
        VALID_LANGUAGES,
        None,
    );

    let theme = clean_choice(
        getter("theme").as_ref().and_then(|v| v.as_str()),
        VALID_THEMES,
        None,
    );

    let minimize_to_tray = getter("minimizeToTray").and_then(|v| v.as_bool());

    let dep_mode = getter("depMode")
        .and_then(|v| {
            clean_choice(
                v.as_str(),
                VALID_DEP_MODES,
                Some(defaults.dep_mode.as_str()),
            )
        })
        .unwrap_or_else(|| defaults.dep_mode.clone());

    let dep_overrides =
        clean_dep_overrides(getter("depOverrides").and_then(|v| {
            serde_json::from_value::<std::collections::HashMap<String, String>>(v).ok()
        }));

    let setup_completed = getter("setupCompleted")
        .and_then(|v| v.as_bool())
        .unwrap_or(defaults.setup_completed);

    let advanced = getter("advanced")
        .and_then(|v| serde_json::from_value::<AdvancedOptions>(v).ok())
        .map(clean_advanced_options)
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

    Ok(parse_settings(|key| value.get(key).cloned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map, Value};

    fn parse_from(value: Value) -> AppSettings {
        let object = value.as_object().cloned().unwrap_or_else(Map::new);
        parse_settings(|key| object.get(key).cloned())
    }

    #[test]
    fn parse_settings_rejects_invalid_download_path() {
        let settings = parse_from(json!({ "downloadPath": "../relative" }));
        assert_eq!(settings.download_path, default_download_path_or_empty());
    }

    #[test]
    fn parse_settings_clamps_large_max_concurrent_without_truncating() {
        let settings = parse_from(json!({ "maxConcurrent": 4294967296_u64 }));
        assert_eq!(settings.max_concurrent, 10);
    }

    #[test]
    fn parse_settings_rejects_invalid_filename_template() {
        let settings = parse_from(json!({ "filenameTemplate": "../%(title)s.%(ext)s" }));
        assert_eq!(
            settings.filename_template,
            AppSettings::default().filename_template
        );
    }

    #[test]
    fn parse_settings_rejects_invalid_cookie_browser() {
        let settings = parse_from(json!({ "cookieBrowser": "chrome\n--output=/tmp/x" }));
        assert_eq!(settings.cookie_browser, None);
    }

    #[test]
    fn parse_settings_rejects_invalid_dep_mode() {
        let settings = parse_from(json!({ "depMode": "network" }));
        assert_eq!(settings.dep_mode, AppSettings::default().dep_mode);
    }

    #[test]
    fn parse_settings_filters_invalid_dep_overrides() {
        let settings = parse_from(json!({
            "depOverrides": {
                "yt-dlp": "systemPath",
                "ffmpeg": "external",
                "deno": "appManaged",
                "curl": "systemPath"
            }
        }));

        assert_eq!(
            settings.dep_overrides.get("yt-dlp").map(String::as_str),
            Some("systemPath")
        );
        assert_eq!(
            settings.dep_overrides.get("deno").map(String::as_str),
            Some("appManaged")
        );
        assert!(!settings.dep_overrides.contains_key("ffmpeg"));
        assert!(!settings.dep_overrides.contains_key("curl"));
    }

    #[test]
    fn parse_settings_rejects_invalid_language() {
        let settings = parse_from(json!({ "language": "ko\n<script>" }));
        assert_eq!(settings.language, None);
    }

    #[test]
    fn parse_settings_rejects_invalid_theme() {
        let settings = parse_from(json!({ "theme": "dark<script>" }));
        assert_eq!(settings.theme, None);
    }

    #[test]
    fn parse_settings_rejects_invalid_advanced_values() {
        let settings = parse_from(json!({
            "advanced": {
                "subLangs": "en;rm -rf",
                "sponsorblockMode": "delete",
                "sponsorblockCategories": ["sponsor", "bad;cat"],
                "videoCodec": "h265;run",
                "limitRate": "999999999999999999999999999999999999999999999999999999999999999G",
                "mergeOutputFormat": "mov",
                "remuxVideo": "avi",
                "convertSubs": "ssa",
                "downloadSections": "abc",
                "proxy": "javascript:alert(1)"
            }
        }));

        let defaults = AdvancedOptions::default();
        assert_eq!(settings.advanced.sub_langs, defaults.sub_langs);
        assert_eq!(
            settings.advanced.sponsorblock_mode,
            defaults.sponsorblock_mode
        );
        assert_eq!(
            settings.advanced.sponsorblock_categories,
            defaults.sponsorblock_categories
        );
        assert_eq!(settings.advanced.video_codec, defaults.video_codec);
        assert_eq!(settings.advanced.limit_rate, defaults.limit_rate);
        assert_eq!(
            settings.advanced.merge_output_format,
            defaults.merge_output_format
        );
        assert_eq!(settings.advanced.remux_video, defaults.remux_video);
        assert_eq!(settings.advanced.convert_subs, defaults.convert_subs);
        assert_eq!(
            settings.advanced.download_sections,
            defaults.download_sections
        );
        assert_eq!(settings.advanced.proxy, defaults.proxy);
    }

    #[test]
    fn parse_settings_clamps_advanced_numbers() {
        let settings = parse_from(json!({
            "advanced": {
                "concurrentFragments": 1000,
                "retries": 1000,
                "sleepInterval": 999999
            }
        }));

        assert_eq!(settings.advanced.concurrent_fragments, 16);
        assert_eq!(settings.advanced.retries, Some(100));
        assert_eq!(settings.advanced.sleep_interval, 86_400);
    }
}
