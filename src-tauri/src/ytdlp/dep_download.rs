use super::types::{DepInstallEvent, DepInstallStage};
use crate::modules::types::AppError;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

/// Ensure the `app_data_dir/bin/` directory exists and return its path.
pub fn ensure_bin_dir(app: &AppHandle) -> Result<PathBuf, AppError> {
    let app_data = app.path().app_data_dir().map_err(|e| {
        AppError::DependencyInstallError(format!("Failed to get app data dir: {}", e))
    })?;
    let bin_dir = app_data.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| {
        AppError::DependencyInstallError(format!("Failed to create bin dir: {}", e))
    })?;
    Ok(bin_dir)
}

/// Download a file from `url` to `dest_dir/temp_name`, emitting progress events.
pub async fn download_file(
    url: &str,
    dest_dir: &Path,
    temp_name: &str,
    app: &AppHandle,
    dep_name: &str,
) -> Result<PathBuf, AppError> {
    let dest_path = dest_dir.join(temp_name);

    let response = reqwest::get(url)
        .await
        .map_err(|e| AppError::DependencyInstallError(format!("Download request failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(AppError::DependencyInstallError(format!(
            "Download failed with status: {}",
            response.status()
        )));
    }

    let total_size = response.content_length();
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(&dest_path)
        .await
        .map_err(|e| AppError::DependencyInstallError(format!("Failed to create file: {}", e)))?;

    let mut downloaded: u64 = 0;
    let mut last_emit_percent: f32 = -1.0;

    use tokio::io::AsyncWriteExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AppError::DependencyInstallError(format!("Download stream error: {}", e))
        })?;
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::DependencyInstallError(format!("Write error: {}", e)))?;

        downloaded += chunk.len() as u64;

        let percent = if let Some(total) = total_size {
            (downloaded as f32 / total as f32 * 100.0).min(100.0)
        } else {
            0.0
        };

        // Emit progress every 2% or at completion
        if (percent - last_emit_percent).abs() >= 2.0 || downloaded == total_size.unwrap_or(0) {
            last_emit_percent = percent;
            let _ = app.emit(
                "dep-install-event",
                DepInstallEvent {
                    dep_name: dep_name.to_string(),
                    stage: DepInstallStage::Downloading,
                    percent,
                    bytes_downloaded: downloaded,
                    bytes_total: total_size,
                    message: None,
                },
            );
        }
    }

    file.flush()
        .await
        .map_err(|e| AppError::DependencyInstallError(format!("Flush error: {}", e)))?;

    Ok(dest_path)
}

/// Verify SHA256 hash of a file against expected hash.
pub async fn verify_sha256(file_path: &Path, expected_hash: &str) -> Result<(), AppError> {
    let path = file_path.to_path_buf();
    let expected = expected_hash.to_lowercase();

    let actual = tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let mut file = std::fs::File::open(&path).map_err(|e| {
            AppError::ChecksumError(format!("Failed to open file for hashing: {}", e))
        })?;
        let mut hasher = Sha256::new();
        std::io::copy(&mut file, &mut hasher)
            .map_err(|e| AppError::ChecksumError(format!("Hash read error: {}", e)))?;
        let hash = hasher.finalize();
        Ok(hex::encode(hash))
    })
    .await
    .map_err(|e| AppError::ChecksumError(format!("Hash task failed: {}", e)))??;

    if actual != expected {
        return Err(AppError::ChecksumError(format!(
            "Checksum mismatch: expected {}, got {}",
            expected, actual
        )));
    }
    Ok(())
}

/// Fetch and parse the SHA2-256SUMS file from yt-dlp releases.
pub async fn fetch_ytdlp_checksums() -> Result<Vec<(String, String)>, AppError> {
    let url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";
    let text = reqwest::get(url)
        .await
        .map_err(|e| AppError::DependencyInstallError(format!("Failed to fetch checksums: {}", e)))?
        .text()
        .await
        .map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to read checksums: {}", e))
        })?;

    let mut results = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.splitn(2, |c: char| c.is_whitespace()).collect();
        if parts.len() == 2 {
            let hash = parts[0].trim().to_string();
            let name = parts[1].trim().trim_start_matches('*').to_string();
            results.push((name, hash));
        }
    }
    Ok(results)
}

/// Extract a zip archive, finding the specified binary inside.
pub async fn extract_zip(
    archive_path: &Path,
    dest_dir: &Path,
    binary_names: &[&str],
) -> Result<Vec<PathBuf>, AppError> {
    let archive = archive_path.to_path_buf();
    let dest = dest_dir.to_path_buf();
    let names: Vec<String> = binary_names.iter().map(|s| s.to_string()).collect();

    tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>, AppError> {
        let file = std::fs::File::open(&archive)
            .map_err(|e| AppError::DependencyInstallError(format!("Failed to open zip: {}", e)))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| AppError::DependencyInstallError(format!("Failed to read zip: {}", e)))?;

        let mut extracted = Vec::new();

        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| AppError::DependencyInstallError(format!("Zip entry error: {}", e)))?;

            let entry_name = entry.name().to_string();
            let file_name = Path::new(&entry_name)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            if names.contains(&file_name) {
                let out_path = dest.join(&file_name);
                let mut out_file = std::fs::File::create(&out_path).map_err(|e| {
                    AppError::DependencyInstallError(format!(
                        "Failed to create extracted file: {}",
                        e
                    ))
                })?;
                std::io::copy(&mut entry, &mut out_file).map_err(|e| {
                    AppError::DependencyInstallError(format!("Failed to extract file: {}", e))
                })?;
                extracted.push(out_path);
            }
        }

        Ok(extracted)
    })
    .await
    .map_err(|e| AppError::DependencyInstallError(format!("Extract task failed: {}", e)))?
}

/// Extract a tar.gz archive, finding the specified binaries inside.
pub async fn extract_tar_gz(
    archive_path: &Path,
    dest_dir: &Path,
    binary_names: &[&str],
) -> Result<Vec<PathBuf>, AppError> {
    let archive = archive_path.to_path_buf();
    let dest = dest_dir.to_path_buf();
    let names: Vec<String> = binary_names.iter().map(|s| s.to_string()).collect();

    tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>, AppError> {
        let file = std::fs::File::open(&archive).map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to open tar.gz: {}", e))
        })?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);

        let mut extracted = Vec::new();

        for entry_result in tar
            .entries()
            .map_err(|e| AppError::DependencyInstallError(format!("Tar entries error: {}", e)))?
        {
            let mut entry = entry_result
                .map_err(|e| AppError::DependencyInstallError(format!("Tar entry error: {}", e)))?;

            let path = entry
                .path()
                .map_err(|e| AppError::DependencyInstallError(format!("Tar path error: {}", e)))?
                .to_path_buf();

            let file_name = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            if names.contains(&file_name) {
                let out_path = dest.join(&file_name);
                let mut out_file = std::fs::File::create(&out_path).map_err(|e| {
                    AppError::DependencyInstallError(format!(
                        "Failed to create extracted file: {}",
                        e
                    ))
                })?;
                std::io::copy(&mut entry, &mut out_file).map_err(|e| {
                    AppError::DependencyInstallError(format!("Failed to extract file: {}", e))
                })?;
                extracted.push(out_path);
            }
        }

        Ok(extracted)
    })
    .await
    .map_err(|e| AppError::DependencyInstallError(format!("Extract task failed: {}", e)))?
}

/// Extract a tar.xz archive, finding the specified binaries inside.
pub async fn extract_tar_xz(
    archive_path: &Path,
    dest_dir: &Path,
    binary_names: &[&str],
) -> Result<Vec<PathBuf>, AppError> {
    let archive = archive_path.to_path_buf();
    let dest = dest_dir.to_path_buf();
    let names: Vec<String> = binary_names.iter().map(|s| s.to_string()).collect();

    tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>, AppError> {
        let file = std::fs::File::open(&archive).map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to open tar.xz: {}", e))
        })?;
        let decoder = xz2::read::XzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);

        let mut extracted = Vec::new();

        for entry_result in tar
            .entries()
            .map_err(|e| AppError::DependencyInstallError(format!("Tar entries error: {}", e)))?
        {
            let mut entry = entry_result
                .map_err(|e| AppError::DependencyInstallError(format!("Tar entry error: {}", e)))?;

            let path = entry
                .path()
                .map_err(|e| AppError::DependencyInstallError(format!("Tar path error: {}", e)))?
                .to_path_buf();

            let file_name = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();

            if names.contains(&file_name) {
                let out_path = dest.join(&file_name);
                let mut out_file = std::fs::File::create(&out_path).map_err(|e| {
                    AppError::DependencyInstallError(format!(
                        "Failed to create extracted file: {}",
                        e
                    ))
                })?;
                std::io::copy(&mut entry, &mut out_file).map_err(|e| {
                    AppError::DependencyInstallError(format!("Failed to extract file: {}", e))
                })?;
                extracted.push(out_path);
            }
        }

        Ok(extracted)
    })
    .await
    .map_err(|e| AppError::DependencyInstallError(format!("Extract task failed: {}", e)))?
}

/// Set executable permission on Unix platforms (chmod 755).
#[cfg(unix)]
pub fn set_executable(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(path, perms)
        .map_err(|e| AppError::DependencyInstallError(format!("Failed to set executable: {}", e)))
}

#[cfg(not(unix))]
pub fn set_executable(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

/// Remove macOS quarantine attribute from downloaded binaries.
#[cfg(target_os = "macos")]
pub fn remove_quarantine(path: &Path) -> Result<(), AppError> {
    let _ = std::process::Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(path)
        .output();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn remove_quarantine(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

/// Atomically move a temporary file to its final path.
pub fn finalize_binary(temp_path: &Path, final_path: &Path) -> Result<(), AppError> {
    // Remove existing binary if present
    if final_path.exists() {
        std::fs::remove_file(final_path).map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to remove old binary: {}", e))
        })?;
    }
    std::fs::rename(temp_path, final_path)
        .map_err(|e| AppError::DependencyInstallError(format!("Failed to finalize binary: {}", e)))
}

/// Get the version string from a binary by running it with a version flag.
pub async fn get_binary_version(binary_path: &Path, version_flag: &str) -> Option<String> {
    let mut cmd = tokio::process::Command::new(binary_path);
    cmd.arg(version_flag);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .ok()?
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.lines().next().unwrap_or("").trim().to_string())
            .filter(|s| !s.is_empty())
    } else {
        None
    }
}

/// Emit a stage event for dependency installation progress.
pub fn emit_stage(app: &AppHandle, dep_name: &str, stage: DepInstallStage, message: Option<&str>) {
    let _ = app.emit(
        "dep-install-event",
        DepInstallEvent {
            dep_name: dep_name.to_string(),
            stage,
            percent: 0.0,
            bytes_downloaded: 0,
            bytes_total: None,
            message: message.map(|s| s.to_string()),
        },
    );
}
