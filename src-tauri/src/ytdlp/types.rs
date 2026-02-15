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

// === Duplicate Check ===

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCheckResult {
    pub in_history: bool,
    pub in_queue: bool,
    pub history_item: Option<HistoryItem>,
}

// === Settings ===

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
    /// Dependency resolution mode: "external" (app-managed) or "system" (system PATH only)
    pub dep_mode: String,
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
            dep_mode: "external".to_string(),
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
    pub source: DepSource,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
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
