use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use once_cell::sync::Lazy;
use regex::Regex;

// Regex patterns for YouTube URL validation
pub(super) static VIDEO_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"^https?://(?:www\.)?youtube\.com/watch\?v=([a-zA-Z0-9_-]{11})").unwrap(),
        Regex::new(r"^https?://(?:www\.)?youtu\.be/([a-zA-Z0-9_-]{11})").unwrap(),
        Regex::new(r"^https?://(?:www\.)?youtube\.com/shorts/([a-zA-Z0-9_-]{11})").unwrap(),
    ]
});

pub(super) static PLAYLIST_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https?://(?:www\.)?youtube\.com/playlist\?list=([a-zA-Z0-9_-]+)").unwrap()
});

static CHANNEL_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"^https?://(?:www\.)?youtube\.com/channel/([a-zA-Z0-9_-]+)").unwrap(),
        Regex::new(r"^https?://(?:www\.)?youtube\.com/@([a-zA-Z0-9_.%\x{0080}-\x{FFFF}-]+)")
            .unwrap(),
        Regex::new(r"^https?://(?:www\.)?youtube\.com/c/([a-zA-Z0-9_.%\x{0080}-\x{FFFF}-]+)")
            .unwrap(),
    ]
});

/// Validate if a URL is a valid YouTube URL
#[tauri::command]
#[specta::specta]
pub fn validate_url(url: String) -> Result<UrlValidation, AppError> {
    let url = url.trim();

    // Check for video URLs
    for pattern in VIDEO_PATTERNS.iter() {
        if let Some(captures) = pattern.captures(url) {
            let video_id = captures.get(1).unwrap().as_str();
            let normalized = format!("https://www.youtube.com/watch?v={}", video_id);
            return Ok(UrlValidation {
                valid: true,
                url_type: UrlType::Video,
                normalized_url: Some(normalized),
                video_id: Some(video_id.to_string()),
            });
        }
    }

    // Check for playlist URL
    if let Some(captures) = PLAYLIST_PATTERN.captures(url) {
        let playlist_id = captures.get(1).unwrap().as_str();
        let normalized = format!("https://www.youtube.com/playlist?list={}", playlist_id);
        return Ok(UrlValidation {
            valid: true,
            url_type: UrlType::Playlist,
            normalized_url: Some(normalized),
            video_id: None,
        });
    }

    // Check for channel URLs
    for pattern in CHANNEL_PATTERNS.iter() {
        if pattern.is_match(url) {
            return Ok(UrlValidation {
                valid: true,
                url_type: UrlType::Channel,
                normalized_url: Some(url.to_string()),
                video_id: None,
            });
        }
    }

    // No match found
    Ok(UrlValidation {
        valid: false,
        url_type: UrlType::Unknown,
        normalized_url: None,
        video_id: None,
    })
}
