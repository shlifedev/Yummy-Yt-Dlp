use super::types::{DependencyStatus, InstallEvent};
use crate::modules::types::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::ipc::Channel;
use tokio::sync::OnceCell;

static RESOLVED_YTDLP: OnceCell<PathBuf> = OnceCell::const_new();

/// Get the binaries directory inside app data dir
pub fn get_binaries_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("binaries")
}

/// Get yt-dlp binary path (platform-specific extension)
/// Returns the app's local binary path (used for downloads/installs)
pub fn get_ytdlp_path(app_data_dir: &Path) -> PathBuf {
    let binaries_dir = get_binaries_dir(app_data_dir);
    match std::env::consts::OS {
        "windows" => binaries_dir.join("yt-dlp.exe"),
        _ => binaries_dir.join("yt-dlp"),
    }
}

/// Resolve the actual yt-dlp binary to use at runtime (cached after first call).
/// Prefers the app's local binary if it works, otherwise falls back to system PATH.
pub async fn resolve_ytdlp_path(app_data_dir: &Path) -> Result<PathBuf, AppError> {
    // Clone app_data_dir for the async closure
    let app_data_dir = app_data_dir.to_path_buf();
    RESOLVED_YTDLP
        .get_or_try_init(|| async {
            let local_path = get_ytdlp_path(&app_data_dir);
            if local_path.exists()
                && try_get_version(&local_path).await.is_some()
            {
                return Ok(local_path);
            }
            if try_get_version(Path::new("yt-dlp")).await.is_some() {
                return Ok(PathBuf::from("yt-dlp"));
            }
            Err(AppError::BinaryNotFound(
                "yt-dlp not found. Please install via Homebrew (brew install yt-dlp) or click Install.".to_string(),
            ))
        })
        .await
        .cloned()
}

/// Get ffmpeg binary path
pub fn get_ffmpeg_path(app_data_dir: &Path) -> PathBuf {
    let binaries_dir = get_binaries_dir(app_data_dir);
    match std::env::consts::OS {
        "windows" => binaries_dir.join("ffmpeg.exe"),
        _ => binaries_dir.join("ffmpeg"),
    }
}

/// Check if yt-dlp is installed, return version if so.
/// Checks the app's local binary first, then falls back to system PATH.
pub async fn check_ytdlp(app_data_dir: &Path) -> Option<String> {
    // First try the app's local binary
    let ytdlp_path = get_ytdlp_path(app_data_dir);
    if ytdlp_path.exists() {
        if let Some(version) = try_get_version(&ytdlp_path).await {
            return Some(version);
        }
    }

    // Fall back to system PATH (e.g. homebrew, pip)
    try_get_version(Path::new("yt-dlp")).await
}

async fn try_get_version(binary_path: &Path) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(binary_path)
            .arg("--version")
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

/// Check if ffmpeg is installed, return version if so
pub async fn check_ffmpeg(app_data_dir: &Path) -> Option<String> {
    let ffmpeg_path = get_ffmpeg_path(app_data_dir);

    if !ffmpeg_path.exists() {
        return None;
    }

    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(&ffmpeg_path)
            .arg("-version")
            .output(),
    )
    .await
    .ok()?
    .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .and_then(|s| s.lines().next().map(|line| line.to_string()))
    } else {
        None
    }
}

/// Get full dependency status
pub async fn check_dependencies(app_data_dir: &Path) -> DependencyStatus {
    let ytdlp_version = check_ytdlp(app_data_dir).await;
    let ffmpeg_version = check_ffmpeg(app_data_dir).await;

    DependencyStatus {
        ytdlp_installed: ytdlp_version.is_some(),
        ytdlp_version,
        ffmpeg_installed: ffmpeg_version.is_some(),
        ffmpeg_version,
    }
}

/// Download yt-dlp binary from GitHub releases
pub async fn download_ytdlp(
    app_data_dir: &Path,
    on_event: &Channel<InstallEvent>,
) -> Result<(), AppError> {
    let url = match std::env::consts::OS {
        "windows" => "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
        "macos" => "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos",
        _ => "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp",
    };

    let binaries_dir = get_binaries_dir(app_data_dir);
    std::fs::create_dir_all(&binaries_dir)
        .map_err(|e| AppError::Custom(format!("Failed to create binaries directory: {}", e)))?;

    let ytdlp_path = get_ytdlp_path(app_data_dir);

    let _ = on_event.send(InstallEvent::Progress {
        dependency: "yt-dlp".to_string(),
        message: "Downloading yt-dlp...".to_string(),
    });

    let response = reqwest::get(url)
        .await
        .map_err(|e| AppError::NetworkError(format!("Failed to download yt-dlp: {}", e)))?;

    if !response.status().is_success() {
        let _ = on_event.send(InstallEvent::Error {
            dependency: "yt-dlp".to_string(),
            message: format!("HTTP error: {}", response.status()),
        });
        return Err(AppError::DownloadError(format!(
            "Failed to download yt-dlp: HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::DownloadError(format!("Failed to read response: {}", e)))?;

    std::fs::write(&ytdlp_path, &bytes)
        .map_err(|e| AppError::Custom(format!("Failed to write yt-dlp binary: {}", e)))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&ytdlp_path)
            .map_err(|e| AppError::Custom(format!("Failed to get file permissions: {}", e)))?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&ytdlp_path, perms).map_err(|e| {
            AppError::Custom(format!("Failed to set executable permissions: {}", e))
        })?;
    }

    // Remove macOS quarantine attribute so Gatekeeper doesn't block execution
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&ytdlp_path)
            .output();
    }

    let _ = on_event.send(InstallEvent::Completed {
        dependency: "yt-dlp".to_string(),
        message: "yt-dlp installed successfully".to_string(),
    });

    Ok(())
}

/// Download ffmpeg binary
pub async fn download_ffmpeg(
    _app_data_dir: &Path,
    on_event: &Channel<InstallEvent>,
) -> Result<(), AppError> {
    let _ = on_event.send(InstallEvent::Progress {
        dependency: "ffmpeg".to_string(),
        message: "ffmpeg download not yet implemented".to_string(),
    });

    let _ = on_event.send(InstallEvent::Completed {
        dependency: "ffmpeg".to_string(),
        message: "ffmpeg download skipped (not implemented)".to_string(),
    });

    Ok(())
}

/// Install both dependencies
pub async fn install_dependencies(
    app_data_dir: &Path,
    on_event: &Channel<InstallEvent>,
) -> Result<(), AppError> {
    download_ytdlp(app_data_dir, on_event).await?;
    download_ffmpeg(app_data_dir, on_event).await?;
    Ok(())
}

/// Update yt-dlp using --update flag
pub async fn update_ytdlp(app_data_dir: &Path) -> Result<String, AppError> {
    let ytdlp_path = resolve_ytdlp_path(app_data_dir).await?;

    let output = tokio::process::Command::new(&ytdlp_path)
        .arg("--update")
        .output()
        .await
        .map_err(|e| AppError::Custom(format!("Failed to update yt-dlp: {}", e)))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Custom(format!("Update failed: {}", stderr)))
    }
}
