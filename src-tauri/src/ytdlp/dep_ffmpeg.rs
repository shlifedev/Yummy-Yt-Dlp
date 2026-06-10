use super::dep_download::*;
use super::types::DepInstallStage;
use crate::modules::logger;
use crate::modules::types::AppError;
use tauri::AppHandle;

/// Where to find the SHA-256 checksum for an ffmpeg archive.
/// The two upstreams expose checksums differently:
/// - vanloctech (macOS) ships a sibling `<asset>.sha256` (single-hash) file.
/// - BtbN (Windows/Linux) ships one combined `checksums.sha256` manifest listing every asset.
enum ChecksumSource {
    /// Sibling file whose entire body is the hash for this one asset.
    Sibling(&'static str),
    /// Combined manifest URL plus the archive filename to look up inside it.
    Manifest {
        url: &'static str,
        filename: &'static str,
    },
}

struct DownloadInfo {
    url: &'static str,
    format: ArchiveFormat,
    checksum: ChecksumSource,
}

/// Get ffmpeg download URL, archive format, and checksum source for the current platform.
fn get_download_info() -> Result<DownloadInfo, AppError> {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            Ok(DownloadInfo {
                url: "https://github.com/vanloctech/ffmpeg-macos/releases/latest/download/ffmpeg-macos-arm64.tar.gz",
                format: ArchiveFormat::TarGz,
                checksum: ChecksumSource::Sibling(
                    "https://github.com/vanloctech/ffmpeg-macos/releases/latest/download/ffmpeg-macos-arm64.tar.gz.sha256",
                ),
            })
        } else {
            Ok(DownloadInfo {
                url: "https://github.com/vanloctech/ffmpeg-macos/releases/latest/download/ffmpeg-macos-x64.tar.gz",
                format: ArchiveFormat::TarGz,
                checksum: ChecksumSource::Sibling(
                    "https://github.com/vanloctech/ffmpeg-macos/releases/latest/download/ffmpeg-macos-x64.tar.gz.sha256",
                ),
            })
        }
    } else if cfg!(target_os = "windows") {
        Ok(DownloadInfo {
            url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
            format: ArchiveFormat::Zip,
            checksum: ChecksumSource::Manifest {
                url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256",
                filename: "ffmpeg-master-latest-win64-gpl.zip",
            },
        })
    } else {
        // Linux
        if cfg!(target_arch = "aarch64") {
            Ok(DownloadInfo {
                url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                format: ArchiveFormat::TarXz,
                checksum: ChecksumSource::Manifest {
                    url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256",
                    filename: "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                },
            })
        } else {
            Ok(DownloadInfo {
                url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
                format: ArchiveFormat::TarXz,
                checksum: ChecksumSource::Manifest {
                    url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/checksums.sha256",
                    filename: "ffmpeg-master-latest-linux64-gpl.tar.xz",
                },
            })
        }
    }
}

enum ArchiveFormat {
    Zip,
    TarGz,
    TarXz,
}

/// Verify a downloaded ffmpeg archive against its published checksum.
///
/// Availability-first, but fail-closed once we actually have a hash:
/// - checksum file fetch fails (404, network, no matching entry) -> warn and continue (Ok);
///   these "latest" rolling releases occasionally lag their checksum publication.
/// - checksum fetched but the archive doesn't match -> Err (caller deletes the archive and aborts).
async fn verify_ffmpeg_checksum(
    archive_path: &std::path::Path,
    source: &ChecksumSource,
) -> Result<(), AppError> {
    let expected = match source {
        ChecksumSource::Sibling(url) => fetch_sha256sum(url).await.map(Some),
        ChecksumSource::Manifest { url, filename } => fetch_checksum_for(url, filename).await,
    };

    match expected {
        Ok(Some(hash)) => {
            verify_sha256(archive_path, &hash).await?;
            logger::info_cat("dependency", "ffmpeg checksum verified");
            Ok(())
        }
        Ok(None) => {
            logger::warn_cat(
                "dependency",
                "ffmpeg checksum manifest had no entry for this asset; proceeding without verification",
            );
            Ok(())
        }
        Err(e) => {
            logger::warn_cat(
                "dependency",
                &format!(
                    "ffmpeg checksum unavailable ({}); proceeding without verification",
                    e
                ),
            );
            Ok(())
        }
    }
}

/// Get ffmpeg/ffprobe binary names for the current platform.
fn get_binary_names() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["ffmpeg.exe", "ffprobe.exe"]
    } else {
        &["ffmpeg", "ffprobe"]
    }
}

/// Install ffmpeg by downloading from GitHub.
pub async fn install_ffmpeg(app: &AppHandle) -> Result<String, AppError> {
    // Serialize against any concurrent ffmpeg install/update/delete so they don't
    // corrupt the shared archive temp file or race the extracted binaries.
    let _lock = lock_dependency("ffmpeg").await;
    let bin_dir = ensure_bin_dir(app)?;
    let info = get_download_info()?;
    let binary_names = get_binary_names();

    let temp_archive = format!(
        "ffmpeg_archive.{}",
        match info.format {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::TarGz => "tar.gz",
            ArchiveFormat::TarXz => "tar.xz",
        }
    );

    // Download
    let archive_path = download_file(info.url, &bin_dir, &temp_archive, app, "ffmpeg").await?;

    // Verify checksum before extracting. Mismatch is fatal (delete + abort); an unavailable
    // checksum only warns so a rolling "latest" release that hasn't published one yet still works.
    emit_stage(
        app,
        "ffmpeg",
        DepInstallStage::Verifying,
        Some("Verifying checksum..."),
    );
    if let Err(e) = verify_ffmpeg_checksum(&archive_path, &info.checksum).await {
        let _ = tokio::fs::remove_file(&archive_path).await;
        return Err(e);
    }

    // Extract
    emit_stage(
        app,
        "ffmpeg",
        DepInstallStage::Extracting,
        Some("Extracting ffmpeg..."),
    );

    let extracted = match info.format {
        ArchiveFormat::Zip => extract_zip(&archive_path, &bin_dir, binary_names).await?,
        ArchiveFormat::TarGz => extract_tar_gz(&archive_path, &bin_dir, binary_names).await?,
        ArchiveFormat::TarXz => extract_tar_xz(&archive_path, &bin_dir, binary_names).await?,
    };

    if extracted.is_empty() {
        // Clean up archive
        let _ = tokio::fs::remove_file(&archive_path).await;
        return Err(AppError::DependencyInstallError(
            "ffmpeg binary not found in archive".to_string(),
        ));
    }

    // Set executable + remove quarantine for each extracted binary
    for path in &extracted {
        set_executable(path)?;
        remove_quarantine(path)?;
    }

    // Clean up archive
    let _ = tokio::fs::remove_file(&archive_path).await;

    // Verify installation
    emit_stage(
        app,
        "ffmpeg",
        DepInstallStage::Completing,
        Some("Verifying installation..."),
    );
    let ffmpeg_bin = if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let ffmpeg_path = bin_dir.join(ffmpeg_bin);
    // The archive checksum was verified above when one was available. A successful
    // `ffmpeg -version` is the final sanity gate (and the only integrity check when the
    // checksum couldn't be fetched). If the extracted binary does not run, treat the
    // install as failed and remove the broken binaries so a later dependency check does
    // not report them as installed.
    let version = match get_binary_version(&ffmpeg_path, "-version").await {
        Some(v) => v,
        None => {
            for path in &extracted {
                let _ = tokio::fs::remove_file(path).await;
            }
            return Err(AppError::DependencyInstallError(
                "ffmpeg installed but failed to run (-version); the download may be corrupt"
                    .to_string(),
            ));
        }
    };

    emit_stage(
        app,
        "ffmpeg",
        DepInstallStage::Completing,
        Some("ffmpeg installed"),
    );

    Ok(version)
}

/// Get the latest ffmpeg version info.
pub async fn get_latest_version() -> Result<String, AppError> {
    // BtbN builds use rolling "latest" tag, so we just return a placeholder.
    // For vanloctech/ffmpeg-macos, check the latest release.
    if cfg!(target_os = "macos") {
        let client = reqwest::Client::new();
        let resp = client
            .get("https://api.github.com/repos/vanloctech/ffmpeg-macos/releases/latest")
            .header("User-Agent", "yummy-yt-dlp")
            .send()
            .await
            .map_err(|e| {
                AppError::NetworkError(format!("Failed to check ffmpeg version: {}", e))
            })?;

        let json = resp
            .json::<serde_json::Value>()
            .await
            .map_err(|e| AppError::NetworkError(format!("Failed to parse response: {}", e)))?;

        json["tag_name"]
            .as_str()
            .map(|s: &str| s.to_string())
            .ok_or_else(|| AppError::NetworkError("No tag_name in response".to_string()))
    } else {
        // BtbN uses "latest" rolling release
        Ok("latest".to_string())
    }
}
