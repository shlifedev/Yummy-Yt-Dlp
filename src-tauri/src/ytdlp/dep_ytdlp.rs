use super::dep_download::*;
use super::types::DepInstallStage;
use crate::modules::types::AppError;
use tauri::AppHandle;

/// Get yt-dlp download URL for the current platform.
fn get_download_url() -> &'static str {
    if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    } else if cfg!(target_os = "windows") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
    } else {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
    }
}

/// Get the expected filename in the SHA2-256SUMS file.
fn get_checksum_filename() -> &'static str {
    if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp_linux"
    }
}

/// Get the final binary name for the current platform.
fn get_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

/// Install yt-dlp by downloading from GitHub releases.
pub async fn install_ytdlp(app: &AppHandle) -> Result<String, AppError> {
    // Serialize against any concurrent yt-dlp install/update/delete so they don't
    // corrupt the shared temp file or race the final binary swap.
    let _lock = lock_dependency("yt-dlp").await;
    let bin_dir = ensure_bin_dir(app)?;
    let url = get_download_url();
    let temp_name = format!("{}.tmp", get_binary_name());

    // Download
    let temp_path = download_file(url, &bin_dir, &temp_name, app, "yt-dlp").await?;

    // Verify SHA256
    emit_stage(
        app,
        "yt-dlp",
        DepInstallStage::Verifying,
        Some("Verifying checksum..."),
    );
    match fetch_ytdlp_checksums().await {
        Ok(checksums) => {
            let expected_name = get_checksum_filename();
            if let Some((_name, hash)) = checksums.iter().find(|(name, _)| name == expected_name) {
                verify_sha256(&temp_path, hash).await?;
            }
        }
        Err(e) => {
            // Non-fatal: log warning but continue
            crate::modules::logger::warn(&format!("Failed to verify yt-dlp checksum: {}", e));
        }
    }

    // Set executable + remove quarantine
    emit_stage(
        app,
        "yt-dlp",
        DepInstallStage::Extracting,
        Some("Setting up binary..."),
    );
    set_executable(&temp_path)?;
    remove_quarantine(&temp_path)?;

    // Finalize
    let final_path = bin_dir.join(get_binary_name());
    finalize_binary(&temp_path, &final_path)?;

    // Verify installation
    emit_stage(
        app,
        "yt-dlp",
        DepInstallStage::Completing,
        Some("Verifying installation..."),
    );
    let version = get_binary_version(&final_path, "--version")
        .await
        .unwrap_or_else(|| "unknown".to_string());

    emit_stage(
        app,
        "yt-dlp",
        DepInstallStage::Completing,
        Some(&format!("yt-dlp {} installed", version)),
    );

    Ok(version)
}

/// Get the latest yt-dlp version from GitHub API.
pub async fn get_latest_version() -> Result<String, AppError> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .header("User-Agent", "modern-ytdlp-gui")
        .send()
        .await
        .map_err(|e| AppError::NetworkError(format!("Failed to check yt-dlp version: {}", e)))?;

    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| AppError::NetworkError(format!("Failed to parse response: {}", e)))?;

    json["tag_name"]
        .as_str()
        .map(|s: &str| s.to_string())
        .ok_or_else(|| AppError::NetworkError("No tag_name in response".to_string()))
}
