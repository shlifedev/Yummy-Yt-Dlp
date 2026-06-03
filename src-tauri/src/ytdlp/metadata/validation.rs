use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use once_cell::sync::Lazy;
use regex::Regex;

// Regex patterns for YouTube URL validation.
// The host group accepts www., music. (YouTube Music) and m. (mobile) subdomains so that
// e.g. music.youtube.com/playlist?list=... is recognised as a playlist instead of falling
// through to single-video handling (which would fail with a JSON parse error).
pub(super) static VIDEO_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"^https?://(?:www\.|music\.|m\.)?youtube\.com/watch\?v=([a-zA-Z0-9_-]{11})")
            .unwrap(),
        Regex::new(r"^https?://(?:www\.)?youtu\.be/([a-zA-Z0-9_-]{11})").unwrap(),
        Regex::new(r"^https?://(?:www\.|music\.|m\.)?youtube\.com/shorts/([a-zA-Z0-9_-]{11})")
            .unwrap(),
    ]
});

pub(super) static PLAYLIST_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^https?://(?:www\.|music\.|m\.)?youtube\.com/playlist\?list=([a-zA-Z0-9_-]+)")
        .unwrap()
});

static CHANNEL_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"^https?://(?:www\.|m\.)?youtube\.com/channel/([a-zA-Z0-9_-]+)").unwrap(),
        Regex::new(r"^https?://(?:www\.|m\.)?youtube\.com/@([a-zA-Z0-9_.%\x{0080}-\x{FFFF}-]+)")
            .unwrap(),
        Regex::new(r"^https?://(?:www\.|m\.)?youtube\.com/c/([a-zA-Z0-9_.%\x{0080}-\x{FFFF}-]+)")
            .unwrap(),
    ]
});

/// Validate if a URL is a valid YouTube URL
#[tauri::command]
#[specta::specta]
pub fn validate_url(url: String) -> Result<UrlValidation, AppError> {
    // Basic security validation (scheme, SSRF protection)
    let url = match crate::ytdlp::security::sanitize_url(&url) {
        Ok(u) => u,
        Err(_) => {
            return Ok(UrlValidation {
                valid: false,
                url_type: UrlType::Unknown,
                normalized_url: None,
                video_id: None,
            });
        }
    };
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

    // 정형 패턴(YouTube)에 안 잡히는 URL은 형식만 통과시키고, 단일 영상인지
    // 재생목록/채널인지는 yt-dlp가 판단하도록 Unknown으로 넘긴다(detect_url_type).
    // sanitize_url이 이미 http/https + SSRF를 검증했으므로 여기선 스킴만 확인한다.
    let lower = url.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(UrlValidation {
            valid: true,
            url_type: UrlType::Unknown,
            normalized_url: Some(url.to_string()),
            video_id: None,
        });
    }

    Ok(UrlValidation {
        valid: false,
        url_type: UrlType::Unknown,
        normalized_url: None,
        video_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_and_mobile_playlists_are_playlists() {
        for url in [
            "https://music.youtube.com/playlist?list=PLabcdef",
            "https://m.youtube.com/playlist?list=PLabcdef",
            "https://www.youtube.com/playlist?list=PLabcdef",
        ] {
            let v = validate_url(url.to_string()).unwrap();
            assert!(v.valid, "{url} should be valid");
            assert!(
                matches!(v.url_type, UrlType::Playlist),
                "{url} should be a playlist, got {:?}",
                v.url_type
            );
        }
    }

    #[test]
    fn music_and_mobile_videos_keep_video_id() {
        for url in [
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            let v = validate_url(url.to_string()).unwrap();
            assert!(
                matches!(v.url_type, UrlType::Video),
                "{url} should be a video, got {:?}",
                v.url_type
            );
            assert_eq!(v.video_id.as_deref(), Some("dQw4w9WgXcQ"), "{url}");
        }
    }
}
