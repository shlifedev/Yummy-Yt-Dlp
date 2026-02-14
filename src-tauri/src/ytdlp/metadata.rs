use super::binary;
use super::types::*;
use crate::modules::types::AppError;
use once_cell::sync::Lazy;
use regex::Regex;
use tauri::AppHandle;

// Regex patterns for YouTube URL validation
static VIDEO_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"^https?://(?:www\.)?youtube\.com/watch\?v=([a-zA-Z0-9_-]{11})").unwrap(),
        Regex::new(r"^https?://(?:www\.)?youtu\.be/([a-zA-Z0-9_-]{11})").unwrap(),
        Regex::new(r"^https?://(?:www\.)?youtube\.com/shorts/([a-zA-Z0-9_-]{11})").unwrap(),
    ]
});

static PLAYLIST_PATTERN: Lazy<Regex> = Lazy::new(|| {
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
        });
    }

    // Check for channel URLs
    for pattern in CHANNEL_PATTERNS.iter() {
        if pattern.is_match(url) {
            return Ok(UrlValidation {
                valid: true,
                url_type: UrlType::Channel,
                normalized_url: Some(url.to_string()),
            });
        }
    }

    // No match found
    Ok(UrlValidation {
        valid: false,
        url_type: UrlType::Unknown,
        normalized_url: None,
    })
}

/// Fetch video metadata using yt-dlp --dump-json
#[tauri::command]
#[specta::specta]
pub async fn fetch_video_info(app: AppHandle, url: String) -> Result<VideoInfo, AppError> {
    let ytdlp_path = binary::resolve_ytdlp_path().await?;
    let settings = super::settings::get_settings(&app).unwrap_or_default();

    // Run yt-dlp with --dump-json
    let mut cmd = tokio::process::Command::new(&ytdlp_path);
    cmd.arg("--dump-json").arg("--no-playlist");
    if let Some(browser) = &settings.cookie_browser {
        cmd.arg("--cookies-from-browser").arg(browser);
    }
    cmd.arg(&url);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| AppError::MetadataError(format!("Failed to execute yt-dlp: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if stderr.contains("Private video") || stderr.contains("Sign in") {
            return Err(AppError::MetadataError(
                "비공개 비디오입니다. 접근할 수 없습니다.".to_string(),
            ));
        }
        if stderr.contains("Video unavailable") || stderr.contains("not available") {
            return Err(AppError::MetadataError(
                "이 비디오는 사용할 수 없습니다.".to_string(),
            ));
        }
        if stderr.contains("is not a valid URL") || stderr.contains("Unsupported URL") {
            return Err(AppError::InvalidUrl(
                "지원하지 않는 URL 형식입니다.".to_string(),
            ));
        }
        if stderr.contains("HTTP Error 429") || stderr.contains("Too Many Requests") {
            return Err(AppError::NetworkError(
                "요청이 너무 많습니다. 잠시 후 다시 시도하세요.".to_string(),
            ));
        }
        if stderr.contains("No video formats found") {
            return Err(AppError::MetadataError(
                "비디오 형식을 찾을 수 없습니다. 라이브 스트림일 수 있습니다.".to_string(),
            ));
        }
        if stderr.contains("age") || stderr.contains("confirm your age") {
            return Err(AppError::MetadataError(
                "연령 제한 콘텐츠입니다. 설정에서 쿠키 브라우저를 설정하세요.".to_string(),
            ));
        }

        return Err(AppError::MetadataError(format!(
            "yt-dlp 오류: {}",
            stderr.lines().last().unwrap_or(&stderr)
        )));
    }

    // Parse JSON output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| AppError::MetadataError(format!("Failed to parse JSON: {}", e)))?;

    // Extract video info
    let video_id = json["id"]
        .as_str()
        .ok_or_else(|| AppError::MetadataError("Missing video id".to_string()))?
        .to_string();

    let title = json["title"]
        .as_str()
        .ok_or_else(|| AppError::MetadataError("Missing title".to_string()))?
        .to_string();

    let thumbnail = json["thumbnail"].as_str().unwrap_or("").to_string();

    let duration = json["duration"].as_u64().unwrap_or(0);

    let upload_date = json["upload_date"].as_str().unwrap_or("").to_string();

    let channel = json["channel"]
        .as_str()
        .or_else(|| json["uploader"].as_str())
        .unwrap_or("")
        .to_string();

    let channel_url = json["channel_url"]
        .as_str()
        .or_else(|| json["uploader_url"].as_str())
        .unwrap_or("")
        .to_string();

    let webpage_url = json["webpage_url"].as_str().unwrap_or(&url).to_string();

    let filesize_approx = json["filesize_approx"].as_u64();

    // Extract formats
    let formats = json["formats"]
        .as_array()
        .ok_or_else(|| AppError::MetadataError("Missing formats array".to_string()))?
        .iter()
        .filter_map(|format| {
            let format_id = format["format_id"].as_str()?.to_string();
            let ext = format["ext"].as_str()?.to_string();
            let resolution = format["resolution"].as_str().map(|s| s.to_string());
            let quality_label = format["format_note"].as_str().map(|s| s.to_string());
            let filesize = format["filesize"].as_u64();
            let vcodec = format["vcodec"].as_str().map(|s| s.to_string());
            let acodec = format["acodec"].as_str().map(|s| s.to_string());

            let has_video = vcodec.as_deref() != Some("none");
            let has_audio = acodec.as_deref() != Some("none");

            Some(FormatInfo {
                format_id,
                ext,
                resolution,
                quality_label,
                filesize,
                vcodec,
                acodec,
                has_video,
                has_audio,
            })
        })
        .collect();

    Ok(VideoInfo {
        url: webpage_url,
        video_id,
        title,
        thumbnail,
        duration,
        upload_date,
        channel,
        channel_url,
        formats,
        filesize_approx,
    })
}

/// Fetch playlist metadata and entries using yt-dlp --flat-playlist
#[tauri::command]
#[specta::specta]
pub async fn fetch_playlist_info(
    app: AppHandle,
    url: String,
    page: u32,
    page_size: u32,
) -> Result<PlaylistResult, AppError> {
    let ytdlp_path = binary::resolve_ytdlp_path().await?;
    let settings = super::settings::get_settings(&app).unwrap_or_default();

    // Run yt-dlp with --flat-playlist --dump-json
    let mut cmd = tokio::process::Command::new(&ytdlp_path);
    cmd.arg("--flat-playlist").arg("--dump-json");
    // Server-side pagination: yt-dlp -I START:END (1-indexed)
    // page_size >= 99999 means "Download All", so skip -I
    if page_size < 99999 {
        let start = page * page_size + 1; // 1-indexed
        let end = start + page_size - 1; // inclusive
        cmd.arg("-I").arg(format!("{}:{}", start, end));
    }
    if let Some(browser) = &settings.cookie_browser {
        cmd.arg("--cookies-from-browser").arg(browser);
    }
    cmd.arg(&url);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| AppError::MetadataError(format!("Failed to execute yt-dlp: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if stderr.contains("Private video") || stderr.contains("Sign in") {
            return Err(AppError::MetadataError(
                "비공개 비디오입니다. 접근할 수 없습니다.".to_string(),
            ));
        }
        if stderr.contains("Video unavailable") || stderr.contains("not available") {
            return Err(AppError::MetadataError(
                "이 비디오는 사용할 수 없습니다.".to_string(),
            ));
        }
        if stderr.contains("is not a valid URL") || stderr.contains("Unsupported URL") {
            return Err(AppError::InvalidUrl(
                "지원하지 않는 URL 형식입니다.".to_string(),
            ));
        }
        if stderr.contains("HTTP Error 429") || stderr.contains("Too Many Requests") {
            return Err(AppError::NetworkError(
                "요청이 너무 많습니다. 잠시 후 다시 시도하세요.".to_string(),
            ));
        }
        if stderr.contains("No video formats found") {
            return Err(AppError::MetadataError(
                "비디오 형식을 찾을 수 없습니다. 라이브 스트림일 수 있습니다.".to_string(),
            ));
        }
        if stderr.contains("age") || stderr.contains("confirm your age") {
            return Err(AppError::MetadataError(
                "연령 제한 콘텐츠입니다. 설정에서 쿠키 브라우저를 설정하세요.".to_string(),
            ));
        }

        return Err(AppError::MetadataError(format!(
            "yt-dlp 오류: {}",
            stderr.lines().last().unwrap_or(&stderr)
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse each line as a JSON object
    let mut all_entries: Vec<serde_json::Value> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(json) => all_entries.push(json),
            Err(e) => {
                eprintln!("Failed to parse line: {} - Error: {}", line, e);
                continue;
            }
        }
    }

    if all_entries.is_empty() {
        if page == 0 {
            return Err(AppError::MetadataError(
                "No entries found in playlist".to_string(),
            ));
        }
        // page > 0 with empty results = end of playlist
        return Ok(PlaylistResult {
            playlist_id: String::new(),
            title: String::new(),
            url: url.clone(),
            video_count: None,
            channel_name: None,
            entries: vec![],
        });
    }

    // Extract playlist-level metadata from the first entry or any entry with playlist info
    let first_entry = &all_entries[0];

    let playlist_id = first_entry["playlist_id"]
        .as_str()
        .or_else(|| {
            // Try to extract from URL
            if let Some(captures) = PLAYLIST_PATTERN.captures(&url) {
                captures.get(1).map(|m| m.as_str())
            } else {
                None
            }
        })
        .unwrap_or("")
        .to_string();

    let title = first_entry["playlist_title"]
        .as_str()
        .or_else(|| first_entry["playlist"].as_str())
        .unwrap_or("Unknown Playlist")
        .to_string();

    let channel_name = first_entry["channel"]
        .as_str()
        .or_else(|| first_entry["playlist_uploader"].as_str())
        .or_else(|| first_entry["uploader"].as_str())
        .map(|s| s.to_string());

    // Try to extract total count from yt-dlp's playlist_count field
    let video_count: Option<u64> =
        first_entry["playlist_count"]
            .as_u64()
            .or(if page_size >= 99999 {
                Some(all_entries.len() as u64) // Full fetch: len() is accurate
            } else {
                None // Paginated: total count unknown
            });

    // Map entries to PlaylistEntry structs
    let mut playlist_entries: Vec<PlaylistEntry> = Vec::new();
    for entry in &all_entries {
        // Extract video_id
        let video_id = entry["id"]
            .as_str()
            .or_else(|| entry["url"].as_str())
            .unwrap_or("")
            .to_string();

        if video_id.is_empty() {
            continue;
        }

        // Construct video URL
        let video_url = if video_id.starts_with("http") {
            video_id.clone()
        } else {
            format!("https://www.youtube.com/watch?v={}", video_id)
        };

        let entry_title = entry["title"].as_str().map(|s| s.to_string());
        let duration = entry["duration"].as_u64();

        // Extract thumbnail
        let thumbnail = entry["thumbnail"]
            .as_str()
            .or_else(|| {
                entry["thumbnails"]
                    .as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|t| t["url"].as_str())
            })
            .map(|s| s.to_string());

        playlist_entries.push(PlaylistEntry {
            url: video_url,
            video_id,
            title: entry_title,
            duration,
            thumbnail,
        });
    }

    // -I flag handles server-side pagination, no skip/take needed
    Ok(PlaylistResult {
        playlist_id,
        title,
        url: url.clone(),
        video_count,
        channel_name,
        entries: playlist_entries,
    })
}
