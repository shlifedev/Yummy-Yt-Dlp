use super::validation::{PLAYLIST_PATTERN, VIDEO_PATTERNS};
use super::{map_stderr_error, run_with_impersonate_fallback};
use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::types::*;
use crate::ytdlp::{binary, security};
use std::time::Duration;
use tauri::AppHandle;

/// Timeout for metadata fetch operations (2 minutes)
const METADATA_TIMEOUT: Duration = Duration::from_secs(120);

/// Fetch video metadata using yt-dlp --dump-json
#[tauri::command]
#[specta::specta]
pub async fn fetch_video_info(app: AppHandle, url: String) -> Result<VideoInfo, AppError> {
    let url = security::sanitize_url(&url)?;
    logger::info_cat("metadata", &format!("Fetching video info: {}", url));
    let ytdlp_path = binary::resolve_ytdlp_path_with_app(&app).await?;
    let settings = crate::ytdlp::settings::get_settings(&app).unwrap_or_default();

    // Run yt-dlp with --dump-json
    let build_cmd = |impersonate: bool| {
        let mut cmd = binary::command_with_path_app(&ytdlp_path, &app);
        cmd.arg("--dump-json").arg("--no-playlist");
        cmd.arg("--encoding").arg("UTF-8");
        if let Some(browser) = &settings.cookie_browser {
            if security::sanitize_cookie_browser(browser).is_ok() {
                cmd.arg("--cookies-from-browser").arg(browser);
            }
        }
        if impersonate {
            cmd.arg("--impersonate").arg(super::IMPERSONATE_TARGET);
        }
        // `--` ends option parsing so a URL beginning with `-` can never be read as a flag.
        cmd.arg("--").arg(&url);

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd
    };

    let output =
        run_with_impersonate_fallback(build_cmd, METADATA_TIMEOUT, "error.fetchTimeout").await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        logger::error_cat(
            "metadata",
            &format!(
                "fetch_video_info failed: {}",
                security::sanitize_error_message(&stderr)
            ),
        );
        return Err(map_stderr_error(&stderr));
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

    logger::info_cat(
        "metadata",
        &format!("Video info fetched: {} ({})", title, video_id),
    );

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

/// 탭이 지정되지 않은 채널 URL(`@handle`, `/channel/ID`, `/c/`, `/user/`)을 `/videos` 탭으로
/// 고정한다. 채널 홈을 그대로 넘기면 yt-dlp가 Videos/Shorts 탭을 별도 플레이리스트로 다뤄
/// `-I` 페이지네이션이 탭마다 중복 적용되고(페이지당 2배 응답), 탭 수만큼 웹페이지를 더 받는다.
fn normalize_bare_channel_url(url: &str) -> String {
    static BARE_CHANNEL: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(
            r"^https?://(?:www\.|m\.)?youtube\.com/(?:@[\w.-]+|channel/[\w-]+|c/[^/?#]+|user/[^/?#]+)/?$",
        )
        .expect("invalid bare-channel regex")
    });
    if BARE_CHANNEL.is_match(url) {
        format!("{}/videos", url.trim_end_matches('/'))
    } else {
        url.to_string()
    }
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
    let url = security::sanitize_url(&url)?;
    logger::info_cat(
        "metadata",
        &format!("Fetching playlist info: {} (page {})", url, page),
    );
    let ytdlp_path = binary::resolve_ytdlp_path_with_app(&app).await?;
    let settings = crate::ytdlp::settings::get_settings(&app).unwrap_or_default();

    // 탭 없는 채널 URL은 Videos/Shorts 탭을 각각 별도 플레이리스트로 받아 `-I`가 탭마다
    // 적용된다(30개 요청 → 60개 응답, 채널 홈 + 탭별 웹페이지 추가 fetch). /videos로 고정.
    let url = normalize_bare_channel_url(&url);

    // Run yt-dlp with --flat-playlist --dump-json
    let build_cmd = |impersonate: bool| {
        let mut cmd = binary::command_with_path_app(&ytdlp_path, &app);
        cmd.arg("--flat-playlist").arg("--dump-json");
        cmd.arg("--encoding").arg("UTF-8");
        // Server-side pagination: yt-dlp -I START:END (1-indexed)
        // page_size >= 99999 means "Download All", so skip -I
        if page_size < 99999 {
            // Clamp page_size to a sane window and use saturating math so a hostile/huge
            // page or page_size can't overflow u32 when computing the 1-indexed range.
            let page_size = page_size.clamp(1, 500);
            let start = page.saturating_mul(page_size).saturating_add(1); // 1-indexed
            let end = start.saturating_add(page_size - 1); // inclusive
            cmd.arg("-I").arg(format!("{}:{}", start, end));
        }
        if let Some(browser) = &settings.cookie_browser {
            if security::sanitize_cookie_browser(browser).is_ok() {
                cmd.arg("--cookies-from-browser").arg(browser);
            }
        }
        if impersonate {
            cmd.arg("--impersonate").arg(super::IMPERSONATE_TARGET);
        }
        // `--` ends option parsing so a URL beginning with `-` can never be read as a flag.
        cmd.arg("--").arg(&url);

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd
    };

    // Use a longer timeout for playlists (5 minutes) since large playlists take more time
    let output =
        run_with_impersonate_fallback(build_cmd, Duration::from_secs(300), "error.fetchTimeout")
            .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(map_stderr_error(&stderr));
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
                logger::warn_cat(
                    "metadata",
                    &format!("Failed to parse playlist entry JSON: {}", e),
                );
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
        // 항목 URL은 yt-dlp가 준 전체 URL을 신뢰한다(webpage_url > url).
        // YouTube 평탄화 항목은 간혹 id만 오므로, ie_key가 YouTube일 때만
        // watch?v= 형태로 재구성한다. 그 외 사이트는 절대 YouTube URL로 바꾸지 않는다.
        let webpage_url = entry["webpage_url"].as_str();
        let url_field = entry["url"].as_str();
        let id_field = entry["id"].as_str();
        let ie_key = entry["ie_key"]
            .as_str()
            .or_else(|| entry["extractor_key"].as_str())
            .unwrap_or("");
        let is_youtube =
            ie_key.eq_ignore_ascii_case("youtube") || ie_key.eq_ignore_ascii_case("youtubetab");

        let video_url = match (webpage_url, url_field) {
            (Some(u), _) if u.starts_with("http") => u.to_string(),
            (_, Some(u)) if u.starts_with("http") => u.to_string(),
            _ => match id_field {
                // id-form만 있는 경우: YouTube(또는 추출기 미상)일 때만 재구성.
                Some(id) if is_youtube || ie_key.is_empty() => {
                    format!("https://www.youtube.com/watch?v={}", id)
                }
                // 비-YouTube + id-form이면 그대로 둬 yt-dlp가 ie_key로 재해석하게 한다.
                Some(id) => id.to_string(),
                None => String::new(),
            },
        };

        if video_url.is_empty() {
            continue;
        }

        // 식별/중복체크용 id: 순수 id 우선, 없으면 URL로 대체.
        let video_id = id_field
            .map(|s| s.to_string())
            .unwrap_or_else(|| video_url.clone());

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

/// Fetch quick metadata via YouTube oEmbed API (~200ms vs ~12s for yt-dlp)
#[tauri::command]
#[specta::specta]
pub async fn fetch_quick_metadata(url: String) -> Result<QuickMetadata, AppError> {
    let url = security::sanitize_url(&url)?;
    logger::info_cat(
        "metadata",
        &format!("Fetching quick metadata (oEmbed): {}", url),
    );

    // Extract video_id from url
    let video_id = VIDEO_PATTERNS
        .iter()
        .find_map(|p| {
            p.captures(&url)
                .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        })
        .ok_or_else(|| AppError::InvalidUrl("Could not extract video ID".to_string()))?;

    let oembed_url = format!(
        "https://www.youtube.com/oembed?url={}&format=json",
        urlencoding::encode(&url)
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::NetworkError(format!("HTTP client error: {}", e)))?;

    let resp = client
        .get(&oembed_url)
        .send()
        .await
        .map_err(|e| AppError::NetworkError(format!("oEmbed request failed: {}", e)))?;

    if !resp.status().is_success() {
        return Err(AppError::MetadataError(format!(
            "oEmbed returned status {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::MetadataError(format!("Failed to parse oEmbed JSON: {}", e)))?;

    let title = json["title"].as_str().unwrap_or("Unknown").to_string();
    let channel = json["author_name"].as_str().unwrap_or("").to_string();
    let channel_url = json["author_url"].as_str().unwrap_or("").to_string();

    // Use high-quality thumbnail from YouTube
    let thumbnail = format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", video_id);

    logger::info_cat(
        "metadata",
        &format!("Quick metadata fetched: {} ({})", title, video_id),
    );

    Ok(QuickMetadata {
        video_id,
        title,
        channel,
        channel_url,
        thumbnail,
    })
}

/// Determine whether an arbitrary URL is a single video or a playlist/channel.
/// Non-YouTube URLs can't be classified by regex, so we let yt-dlp decide via the
/// top-level `_type` field. `--flat-playlist` + `--playlist-items 1` keeps this fast
/// even for huge channels (no per-item extraction).
#[tauri::command]
#[specta::specta]
pub async fn detect_url_type(app: AppHandle, url: String) -> Result<UrlType, AppError> {
    let url = security::sanitize_url(&url)?;
    logger::info_cat("metadata", &format!("Detecting URL type: {}", url));
    let ytdlp_path = binary::resolve_ytdlp_path_with_app(&app).await?;
    let settings = crate::ytdlp::settings::get_settings(&app).unwrap_or_default();

    let build_cmd = |impersonate: bool| {
        let mut cmd = binary::command_with_path_app(&ytdlp_path, &app);
        cmd.arg("--dump-single-json")
            .arg("--flat-playlist")
            .arg("--playlist-items")
            .arg("1")
            .arg("--encoding")
            .arg("UTF-8");
        if let Some(browser) = &settings.cookie_browser {
            if security::sanitize_cookie_browser(browser).is_ok() {
                cmd.arg("--cookies-from-browser").arg(browser);
            }
        }
        if impersonate {
            cmd.arg("--impersonate").arg(super::IMPERSONATE_TARGET);
        }
        // `--` ends option parsing so a URL beginning with `-` can never be read as a flag.
        cmd.arg("--").arg(&url);

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        cmd
    };

    // Detection only resolves one item, so a tighter timeout than full fetch is fine.
    let output =
        run_with_impersonate_fallback(build_cmd, Duration::from_secs(60), "error.fetchTimeout")
            .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        logger::error_cat(
            "metadata",
            &format!(
                "detect_url_type failed: {}",
                security::sanitize_error_message(&stderr)
            ),
        );
        return Err(map_stderr_error(&stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| AppError::MetadataError(format!("Failed to parse JSON: {}", e)))?;

    let url_type = match json["_type"].as_str() {
        Some("playlist") | Some("multi_video") => UrlType::Playlist,
        _ => UrlType::Video,
    };

    logger::info_cat(
        "metadata",
        &format!("URL type detected: {:?} ({})", url_type, url),
    );
    Ok(url_type)
}

#[cfg(test)]
mod tests {
    use super::normalize_bare_channel_url;

    #[test]
    fn bare_channel_urls_get_videos_tab() {
        for (input, expected) in [
            (
                "https://www.youtube.com/@toothfairy_pf",
                "https://www.youtube.com/@toothfairy_pf/videos",
            ),
            (
                "https://www.youtube.com/@toothfairy_pf/",
                "https://www.youtube.com/@toothfairy_pf/videos",
            ),
            (
                "https://youtube.com/channel/UCkO6FQ8JkmYdyYKnRji8zcA",
                "https://youtube.com/channel/UCkO6FQ8JkmYdyYKnRji8zcA/videos",
            ),
            (
                "https://www.youtube.com/user/somebody",
                "https://www.youtube.com/user/somebody/videos",
            ),
        ] {
            assert_eq!(normalize_bare_channel_url(input), expected);
        }
    }

    #[test]
    fn non_channel_urls_pass_through() {
        for url in [
            "https://www.youtube.com/@toothfairy_pf/shorts",
            "https://www.youtube.com/@toothfairy_pf/videos",
            "https://www.youtube.com/watch?v=abc123",
            "https://www.youtube.com/playlist?list=PL123",
            "https://vimeo.com/12345",
        ] {
            assert_eq!(normalize_bare_channel_url(url), url);
        }
    }
}
