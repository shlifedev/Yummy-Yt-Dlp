use super::types::DependencyStatus;
use crate::modules::types::AppError;
use std::path::Path;
use std::time::Duration;

/// Resolve the yt-dlp binary from system PATH.
pub async fn resolve_ytdlp_path() -> Result<String, AppError> {
    if try_get_version(Path::new("yt-dlp")).await.is_ok() {
        return Ok("yt-dlp".to_string());
    }
    Err(AppError::BinaryNotFound(
        "yt-dlp not found. Please install via your package manager (e.g. brew install yt-dlp)."
            .to_string(),
    ))
}

/// Check if yt-dlp is installed on system PATH, return version if so.
pub async fn check_ytdlp() -> Option<String> {
    try_get_version(Path::new("yt-dlp")).await.ok()
}

/// Try to get version from a binary. Returns Ok(version) or Err(reason).
async fn try_get_version(binary_path: &Path) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(binary_path);
    cmd.arg("--version");

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // PyInstaller binaries (yt-dlp_macos) need time to extract on first run
    let timeout_result = tokio::time::timeout(Duration::from_secs(30), cmd.output()).await;

    let cmd_result = match timeout_result {
        Ok(result) => result,
        Err(_) => {
            return Err(format!("timeout (30s) executing {}", binary_path.display()));
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

/// Check if ffmpeg is installed on system PATH, return version if so.
pub async fn check_ffmpeg() -> Option<String> {
    let mut cmd = tokio::process::Command::new("ffmpeg");
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
pub async fn check_dependencies() -> DependencyStatus {
    let ytdlp_version = check_ytdlp().await;
    let ffmpeg_version = check_ffmpeg().await;

    DependencyStatus {
        ytdlp_installed: ytdlp_version.is_some(),
        ytdlp_version,
        ffmpeg_installed: ffmpeg_version.is_some(),
        ffmpeg_version,
        ytdlp_debug: None,
    }
}

/// Update yt-dlp using --update flag
pub async fn update_ytdlp() -> Result<String, AppError> {
    let ytdlp_path = resolve_ytdlp_path().await?;

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
