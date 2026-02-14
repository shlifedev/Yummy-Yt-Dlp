use super::types::{DependencyStatus, InstallEvent};
use crate::modules::types::AppError;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;
use tauri::ipc::Channel;

// 2-5: RwLock instead of OnceCell for cache invalidation support
static RESOLVED_YTDLP: RwLock<Option<PathBuf>> = RwLock::new(None);

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

/// Resolve the actual yt-dlp binary to use at runtime (cached).
/// Prefers the app's local binary if it works, otherwise falls back to system PATH.
/// Cache can be cleared with `clear_ytdlp_cache()` after installation.
pub async fn resolve_ytdlp_path(app_data_dir: &Path) -> Result<PathBuf, AppError> {
    // Check cache first
    {
        let cache = RESOLVED_YTDLP.read().unwrap_or_else(|e| e.into_inner());
        if let Some(path) = cache.as_ref() {
            return Ok(path.clone());
        }
    }

    // Resolve the path
    let app_data_dir = app_data_dir.to_path_buf();
    let local_path = get_ytdlp_path(&app_data_dir);
    if local_path.exists() && try_get_version(&local_path).await.is_ok() {
        let mut cache = RESOLVED_YTDLP.write().unwrap_or_else(|e| e.into_inner());
        *cache = Some(local_path.clone());
        return Ok(local_path);
    }
    if try_get_version(Path::new("yt-dlp")).await.is_ok() {
        let path = PathBuf::from("yt-dlp");
        let mut cache = RESOLVED_YTDLP.write().unwrap_or_else(|e| e.into_inner());
        *cache = Some(path.clone());
        return Ok(path);
    }
    Err(AppError::BinaryNotFound(
        "yt-dlp not found. Please install via Homebrew (brew install yt-dlp) or click Install."
            .to_string(),
    ))
}

/// Clear the cached yt-dlp path so the next call to `resolve_ytdlp_path` re-resolves.
pub fn clear_ytdlp_cache() {
    let mut cache = RESOLVED_YTDLP.write().unwrap_or_else(|e| e.into_inner());
    *cache = None;
}

/// Get ffmpeg binary path
pub fn get_ffmpeg_path(app_data_dir: &Path) -> PathBuf {
    let binaries_dir = get_binaries_dir(app_data_dir);
    match std::env::consts::OS {
        "windows" => binaries_dir.join("ffmpeg.exe"),
        _ => binaries_dir.join("ffmpeg"),
    }
}

/// Check if yt-dlp is installed, return (version, debug_info).
/// Checks the app's local binary first, then falls back to system PATH.
/// When version is None, debug_info explains why.
pub async fn check_ytdlp(app_data_dir: &Path) -> (Option<String>, Option<String>) {
    let mut debug_lines: Vec<String> = Vec::new();

    // First try the app's local binary
    let ytdlp_path = get_ytdlp_path(app_data_dir);
    let exists = ytdlp_path.exists();
    debug_lines.push(format!("local: {} (exists={})", ytdlp_path.display(), exists));

    if exists {
        // Check file metadata
        if let Ok(meta) = std::fs::metadata(&ytdlp_path) {
            debug_lines.push(format!("  size={} bytes", meta.len()));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                debug_lines.push(format!("  mode={:o}", meta.permissions().mode()));
            }
        }

        match try_get_version(&ytdlp_path).await {
            Ok(version) => return (Some(version), None),
            Err(reason) => {
                debug_lines.push(format!("  FAIL: {}", reason));
            }
        }
    }

    // Fall back to system PATH (e.g. homebrew, pip)
    debug_lines.push("fallback: system PATH 'yt-dlp'".to_string());
    match try_get_version(Path::new("yt-dlp")).await {
        Ok(version) => return (Some(version), None),
        Err(reason) => {
            debug_lines.push(format!("  FAIL: {}", reason));
        }
    }

    (None, Some(debug_lines.join("\n")))
}

/// Try to get version from a binary. Returns Ok(version) or Err(reason).
async fn try_get_version(binary_path: &Path) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(binary_path);
    cmd.arg("--version");

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let timeout_result = tokio::time::timeout(Duration::from_secs(5), cmd.output()).await;

    let cmd_result = match timeout_result {
        Ok(result) => result,
        Err(_) => {
            return Err(format!("timeout (5s) executing {}", binary_path.display()));
        }
    };

    let output = match cmd_result {
        Ok(output) => output,
        Err(e) => {
            return Err(format!("exec error: {} ({})", e, e.kind()));
        }
    };

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("invalid utf8 in stdout: {}", e))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "exit code={}, stderr={}",
            output.status,
            stderr.trim()
        ))
    }
}

/// Check if ffmpeg is installed, return version if so
pub async fn check_ffmpeg(app_data_dir: &Path) -> Option<String> {
    let ffmpeg_path = get_ffmpeg_path(app_data_dir);

    if !ffmpeg_path.exists() {
        return None;
    }

    let mut cmd = tokio::process::Command::new(&ffmpeg_path);
    cmd.arg("-version");

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = tokio::time::timeout(Duration::from_secs(5), cmd.output())
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
    let (ytdlp_version, ytdlp_debug) = check_ytdlp(app_data_dir).await;
    let ffmpeg_version = check_ffmpeg(app_data_dir).await;

    DependencyStatus {
        ytdlp_installed: ytdlp_version.is_some(),
        ytdlp_version,
        ffmpeg_installed: ffmpeg_version.is_some(),
        ffmpeg_version,
        ytdlp_debug,
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

    log::debug!(
        "[binary] download_ytdlp: downloaded {} bytes, writing to {}",
        bytes.len(),
        ytdlp_path.display()
    );

    std::fs::write(&ytdlp_path, &bytes)
        .map_err(|e| AppError::Custom(format!("Failed to write yt-dlp binary: {}", e)))?;

    log::debug!("[binary] download_ytdlp: file written successfully");

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
        log::debug!("[binary] download_ytdlp: chmod 755 applied");
    }

    // Remove macOS quarantine attribute so Gatekeeper doesn't block execution
    #[cfg(target_os = "macos")]
    {
        let xattr_result = std::process::Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(&ytdlp_path)
            .output();
        match &xattr_result {
            Ok(output) => {
                if output.status.success() {
                    log::debug!("[binary] download_ytdlp: xattr quarantine removed");
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    log::warn!(
                        "[binary] download_ytdlp: xattr failed (exit={}): {}",
                        output.status,
                        stderr.trim()
                    );
                }
            }
            Err(e) => {
                log::warn!("[binary] download_ytdlp: xattr command error: {}", e);
            }
        }
    }

    // 2-5: Clear cached path so resolve_ytdlp_path picks up the newly installed binary
    clear_ytdlp_cache();
    log::debug!("[binary] download_ytdlp: cache cleared");

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

    let mut cmd = tokio::process::Command::new(&ytdlp_path);
    cmd.arg("--update");

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = cmd
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
