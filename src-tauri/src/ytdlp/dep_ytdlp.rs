use super::dep_download::*;
use super::types::DepInstallStage;
use crate::modules::logger;
use crate::modules::types::AppError;
use std::path::Path;
use std::time::Duration;
use tauri::AppHandle;

/// yt-dlp ships as a PyInstaller `--onedir` zip per platform: the executable plus
/// an `_internal/` directory. onedir self-extracts once and then launches in
/// ~0.2s, versus ~9s every run for the old `--onefile` build (re-extract + macOS
/// Gatekeeper recheck). We install the whole tree under `bin/ytdlp/`.
///
/// Get the yt-dlp release zip asset name / download URL for the current platform.
fn get_zip_asset() -> &'static str {
    if cfg!(target_os = "macos") {
        "yt-dlp_macos.zip"
    } else if cfg!(target_os = "windows") {
        "yt-dlp_win.zip"
    } else {
        "yt-dlp_linux.zip"
    }
}

fn get_download_url() -> String {
    format!(
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/{}",
        get_zip_asset()
    )
}

/// The executable name *inside* the extracted zip, before we normalize it.
fn get_archived_exe_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp_linux"
    }
}

/// The normalized executable name we rename to inside `bin/ytdlp/`.
fn get_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

/// `bin/ytdlp/` — the directory holding the app-managed onedir yt-dlp.
fn ytdlp_dir(bin_dir: &Path) -> std::path::PathBuf {
    bin_dir.join("ytdlp")
}

/// Best-effort removal of the legacy `--onefile` binary that older installs left
/// directly in `bin/`. Once the onedir tree is in place it's just dead weight and
/// would otherwise still win resolution as the legacy fallback.
fn remove_legacy_binary(bin_dir: &Path) {
    let legacy = bin_dir.join(get_binary_name());
    let _ = std::fs::remove_file(legacy);
}

/// Install yt-dlp by downloading the onedir zip from GitHub releases.
pub async fn install_ytdlp(app: &AppHandle) -> Result<String, AppError> {
    // Serialize against any concurrent yt-dlp install/update/delete so they don't
    // corrupt the shared temp file or race the final directory swap.
    let _lock = lock_dependency("yt-dlp").await;
    let bin_dir = ensure_bin_dir(app)?;
    let url = get_download_url();
    let temp_zip_name = "yt-dlp-onedir.zip.tmp";

    // Download
    let temp_zip = download_file(&url, &bin_dir, temp_zip_name, app, "yt-dlp").await?;

    // Verify SHA256
    emit_stage(
        app,
        "yt-dlp",
        DepInstallStage::Verifying,
        Some("Verifying checksum..."),
    );
    // Fail closed: a binary we cannot verify is never installed. yt-dlp's
    // SHA2-256SUMS lists the zip assets (yt-dlp_macos.zip / yt-dlp_win.zip /
    // yt-dlp_linux.zip), so the happy path is unaffected; only a fetch failure or a
    // missing entry blocks install.
    let checksums = match fetch_ytdlp_checksums().await {
        Ok(c) => c,
        Err(e) => {
            let _ = tokio::fs::remove_file(&temp_zip).await;
            return Err(AppError::ChecksumError(format!(
                "cannot verify yt-dlp checksum: {}",
                e
            )));
        }
    };
    let expected_name = get_zip_asset();
    let hash = match checksums.iter().find(|(name, _)| name == expected_name) {
        Some((_name, hash)) => hash,
        None => {
            let _ = tokio::fs::remove_file(&temp_zip).await;
            return Err(AppError::ChecksumError(format!(
                "no checksum entry found for {}",
                expected_name
            )));
        }
    };
    if let Err(e) = verify_sha256(&temp_zip, hash).await {
        let _ = tokio::fs::remove_file(&temp_zip).await;
        return Err(e);
    }

    // Extract into a staging dir (structure-preserving, zip-slip guarded), normalize
    // the executable name, then atomically swap it in as bin/ytdlp/.
    emit_stage(
        app,
        "yt-dlp",
        DepInstallStage::Extracting,
        Some("Extracting..."),
    );

    let staging = bin_dir.join("ytdlp.staging");
    let _ = std::fs::remove_dir_all(&staging);
    let extract_result = extract_zip_tree(&temp_zip, &staging).await;
    let _ = tokio::fs::remove_file(&temp_zip).await;
    if let Err(e) = extract_result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Normalize the executable name (yt-dlp_macos / yt-dlp_linux -> yt-dlp).
    let archived = staging.join(get_archived_exe_name());
    let normalized = staging.join(get_binary_name());
    if archived != normalized {
        if let Err(e) = std::fs::rename(&archived, &normalized) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(AppError::DependencyInstallError(format!(
                "extracted archive missing expected executable {}: {}",
                get_archived_exe_name(),
                e
            )));
        }
    }
    if let Err(e) = set_executable(&normalized) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let _ = remove_quarantine_recursive(&staging);

    // Verify the STAGED binary before swapping it in: a failed update must never
    // destroy the last working tree. Only spawn errors, non-zero exits, and empty
    // output are fatal — a slow PyInstaller cold start (timeout) gets one retry
    // with a longer window and then installs with a warning rather than discarding
    // a build that may simply be slow to bootstrap.
    emit_stage(
        app,
        "yt-dlp",
        DepInstallStage::Verifying,
        Some("Verifying installation..."),
    );
    let staged_exe = staging.join(get_binary_name());
    let mut probe = probe_binary_version(&staged_exe, "--version", Duration::from_secs(25)).await;
    if matches!(probe, Err(VersionProbeError::Timeout(_))) {
        probe = probe_binary_version(&staged_exe, "--version", Duration::from_secs(60)).await;
    }
    let version = match probe {
        Ok(v) => v,
        Err(VersionProbeError::Timeout(_)) => {
            logger::warn_cat(
                "dependency",
                "yt-dlp version probe timed out twice; keeping the install (cold start can exceed the probe window)",
            );
            "unknown".to_string()
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            let msg = format!(
                "yt-dlp installed but failed to run (--version): {}; the download may be corrupt or incompatible",
                e
            );
            emit_stage(app, "yt-dlp", DepInstallStage::Failed, Some(&msg));
            return Err(AppError::DependencyInstallError(msg));
        }
    };

    // Last-moment gate under the dependency lock: never swap the tree while a
    // download could be running the old yt-dlp. A download started between this
    // check and the rename can still race (TOCTOU), but this closes the realistic
    // startup auto-update vs queue-resume collision.
    if downloads_busy(app) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(downloads_busy_error());
    }

    let final_dir = ytdlp_dir(&bin_dir);
    if let Err(e) = finalize_dir(&staging, &final_dir) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    remove_legacy_binary(&bin_dir);

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
    let resp = short_http_client()
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .header("User-Agent", "yummy-yt-dlp")
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
