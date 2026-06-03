use serde::{Deserialize, Serialize};

// === Video Metadata ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub url: String,
    pub video_id: String,
    pub title: String,
    pub thumbnail: String,
    pub duration: u64,
    pub upload_date: String,
    pub channel: String,
    pub channel_url: String,
    pub formats: Vec<FormatInfo>,
    pub filesize_approx: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FormatInfo {
    pub format_id: String,
    pub ext: String,
    pub resolution: Option<String>,
    pub quality_label: Option<String>,
    pub filesize: Option<u64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub has_video: bool,
    pub has_audio: bool,
}

// === Playlist / Channel ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistResult {
    pub playlist_id: String,
    pub title: String,
    pub url: String,
    pub video_count: Option<u64>,
    pub channel_name: Option<String>,
    pub entries: Vec<PlaylistEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub url: String,
    pub video_id: String,
    pub title: Option<String>,
    pub duration: Option<u64>,
    pub thumbnail: Option<String>,
}

// === Quick Metadata (oEmbed) ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QuickMetadata {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub channel_url: String,
    pub thumbnail: String,
}

// === URL Validation ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UrlValidation {
    pub valid: bool,
    pub url_type: UrlType,
    pub normalized_url: Option<String>,
    pub video_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub enum UrlType {
    #[serde(rename = "video")]
    Video,
    #[serde(rename = "channel")]
    Channel,
    #[serde(rename = "playlist")]
    Playlist,
    #[serde(rename = "unknown")]
    Unknown,
}

// === Download ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub video_url: String,
    pub video_id: String,
    pub title: String,
    pub format_id: String,
    pub quality_label: String,
    pub output_dir: Option<String>,
    pub cookie_browser: Option<String>,
    pub audio_format: Option<String>,
    pub audio_quality: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadStatus::Pending => write!(f, "pending"),
            DownloadStatus::Downloading => write!(f, "downloading"),
            DownloadStatus::Paused => write!(f, "paused"),
            DownloadStatus::Completed => write!(f, "completed"),
            DownloadStatus::Failed => write!(f, "failed"),
            DownloadStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl DownloadStatus {
    pub fn parse(s: &str) -> Self {
        match s {
            "pending" => DownloadStatus::Pending,
            "downloading" => DownloadStatus::Downloading,
            "paused" => DownloadStatus::Paused,
            "completed" => DownloadStatus::Completed,
            "failed" => DownloadStatus::Failed,
            "cancelled" => DownloadStatus::Cancelled,
            unknown => {
                eprintln!(
                    "[DownloadStatus] Unknown status '{}', defaulting to Pending",
                    unknown
                );
                DownloadStatus::Pending
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadTaskInfo {
    pub id: u64,
    pub video_url: String,
    pub video_id: String,
    pub title: String,
    pub format_id: String,
    pub quality_label: String,
    pub output_path: String,
    pub status: DownloadStatus,
    pub progress: f32,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub audio_format: Option<String>,
    pub audio_quality: Option<String>,
}

// Global download event for app-wide event emission
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct GlobalDownloadEvent {
    pub task_id: u64,
    pub event_type: String, // "started", "progress", "completed", "error"
    pub percent: Option<f32>,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<u64>,
    pub message: Option<String>,
}

// === Install ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DependencyStatus {
    pub ytdlp_installed: bool,
    pub ytdlp_version: Option<String>,
    pub ffmpeg_installed: bool,
    pub ffmpeg_version: Option<String>,
    /// Diagnostic info when ytdlp check fails (path tried, error reason)
    pub ytdlp_debug: Option<String>,
}

// === History ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: u64,
    pub video_url: String,
    pub video_id: String,
    pub title: String,
    pub quality_label: String,
    pub format: String,
    pub file_path: String,
    pub file_size: Option<u64>,
    pub downloaded_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryResult {
    pub items: Vec<HistoryItem>,
    pub total_count: u64,
    pub page: u32,
    pub page_size: u32,
}

// === Queue Pagination ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueResult {
    pub items: Vec<DownloadTaskInfo>,
    pub total_count: u64,
    pub page: u32,
    pub page_size: u32,
    pub active_count: u64,
    pub pending_count: u64,
    pub completed_count: u64,
    pub failed_count: u64,
    pub cancelled_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueSummary {
    pub active_items: Vec<DownloadTaskInfo>,
    pub recent_completed: Vec<DownloadTaskInfo>,
    pub active_count: u64,
    pub pending_count: u64,
    pub completed_count: u64,
    pub total_count: u64,
}

// === Duplicate Check ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCheckResult {
    pub in_history: bool,
    pub in_queue: bool,
    pub history_item: Option<HistoryItem>,
    pub file_exists: bool,
}

// === Settings ===

/// Global advanced yt-dlp options exposed via the "Advanced" panel on the download page.
/// All options are global (not per-download) and persisted in settings.json. The container-level
/// `#[serde(default)]` + a custom `Default` impl keeps old settings.json files (and partial objects
/// sent from the frontend) loading cleanly even as new fields are added later.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AdvancedOptions {
    // Subtitles
    pub write_subs: bool,
    pub write_auto_subs: bool,
    pub embed_subs: bool,
    pub sub_langs: String,
    pub convert_subs: String,

    // SponsorBlock
    pub sponsorblock_mode: String, // "off" | "mark" | "remove"
    pub sponsorblock_categories: Vec<String>,

    // Embedding & metadata
    pub embed_thumbnail: bool,
    pub embed_metadata: bool,
    pub embed_chapters: bool,
    pub write_thumbnail: bool,
    pub write_info_json: bool,

    // Format / codec / speed
    pub video_codec: String, // "auto" | "av01" | "vp9" | "h264"
    pub limit_rate: String,

    // Network reliability
    pub concurrent_fragments: u32,
    pub retries: Option<u32>,
    pub sleep_interval: u32,

    // Container
    pub merge_output_format: String, // "" | "mp4" | "mkv" | "webm"
    pub remux_video: String,         // "" | "mp4" | "mkv" | "webm"

    // Sections / chapters
    pub download_sections: String,
    pub split_chapters: bool,

    // Proxy / timestamp / filename
    pub proxy: String,
    pub no_mtime: bool,
    pub restrict_filenames: bool,
}

impl Default for AdvancedOptions {
    fn default() -> Self {
        Self {
            write_subs: false,
            write_auto_subs: false,
            embed_subs: false,
            sub_langs: "en".to_string(),
            convert_subs: String::new(),
            sponsorblock_mode: "off".to_string(),
            sponsorblock_categories: vec!["sponsor".to_string()],
            embed_thumbnail: false,
            embed_metadata: false,
            embed_chapters: false,
            write_thumbnail: false,
            write_info_json: false,
            video_codec: "auto".to_string(),
            limit_rate: String::new(),
            concurrent_fragments: 1,
            retries: None,
            sleep_interval: 0,
            merge_output_format: String::new(),
            remux_video: String::new(),
            download_sections: String::new(),
            split_chapters: false,
            proxy: String::new(),
            no_mtime: false,
            restrict_filenames: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub download_path: String,
    pub default_quality: String,
    pub max_concurrent: u32,
    pub filename_template: String,
    pub cookie_browser: Option<String>,
    pub auto_update_ytdlp: bool,
    pub use_advanced_template: bool,
    pub template_uploader_folder: bool,
    pub template_upload_date: bool,
    pub template_video_id: bool,
    pub language: Option<String>,
    pub theme: Option<String>,
    pub minimize_to_tray: Option<bool>,
    /// Dependency resolution mode: "hybrid" (system first, bundled fallback),
    /// "bundled" (app-managed first), or "system" (system PATH only).
    /// Legacy "external" is treated as "bundled" at resolution time.
    pub dep_mode: String,
    /// Per-dependency source override. Maps a dependency name ("yt-dlp",
    /// "ffmpeg", "deno") to "appManaged" or "systemPath". A dependency without
    /// an entry follows `dep_mode`.
    #[serde(default)]
    pub dep_overrides: std::collections::HashMap<String, String>,
    /// Global advanced download options (subtitles, SponsorBlock, embedding, codec, etc.)
    #[serde(default)]
    pub advanced: AdvancedOptions,
    /// Whether the initial setup wizard has been completed
    pub setup_completed: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            download_path: String::new(),
            default_quality: "1080p".to_string(),
            max_concurrent: 3,
            filename_template: "%(title)s.%(ext)s".to_string(),
            cookie_browser: None,
            auto_update_ytdlp: true,
            use_advanced_template: false,
            template_uploader_folder: false,
            template_upload_date: false,
            template_video_id: false,
            language: None,
            theme: None,
            minimize_to_tray: None,
            dep_mode: "hybrid".to_string(),
            dep_overrides: std::collections::HashMap::new(),
            advanced: AdvancedOptions::default(),
            setup_completed: false,
        }
    }
}

// === Progress ===

#[derive(Debug, Clone)]
pub struct ProgressInfo {
    pub percent: f32,
    pub speed: Option<String>,
    pub eta: Option<String>,
}

// === Dependency Install ===

#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct DepInstallEvent {
    pub dep_name: String,
    pub stage: DepInstallStage,
    pub percent: f32,
    pub bytes_downloaded: u64,
    pub bytes_total: Option<u64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub enum DepInstallStage {
    Downloading,
    Verifying,
    Extracting,
    Completing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FullDependencyStatus {
    pub ytdlp: DepInfo,
    pub ffmpeg: DepInfo,
    pub deno: DepInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DepInfo {
    pub installed: bool,
    pub version: Option<String>,
    /// The source currently active for this dependency (honoring any override).
    pub source: DepSource,
    pub path: Option<String>,
    /// Whether an app-managed copy exists on disk, independent of which source
    /// is active. Drives the per-item source toggle in the UI.
    #[serde(default)]
    pub app_available: bool,
    /// Whether a system-PATH copy is discoverable, independent of the active source.
    #[serde(default)]
    pub system_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum DepSource {
    AppManaged,
    SystemPath,
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DepUpdateInfo {
    pub current_version: Option<String>,
    pub latest_version: String,
    pub update_available: bool,
}

// === Logs ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: i64,
    pub timestamp: i64,
    pub level: String,
    pub category: String,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogQueryResult {
    pub items: Vec<LogEntry>,
    pub total_count: u64,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogStats {
    pub total_count: u64,
    pub error_count: u64,
    pub warn_count: u64,
    pub info_count: u64,
}

#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct NewLogEvent {
    pub entry: LogEntry,
}
