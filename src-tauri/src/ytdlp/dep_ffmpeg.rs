use super::dep_download::*;
use super::types::DepInstallStage;
use crate::modules::types::AppError;
use tauri::AppHandle;

/// Get ffmpeg download URL and archive format for the current platform.
fn get_download_info() -> Result<(&'static str, ArchiveFormat), AppError> {
    if cfg!(target_os = "macos") {
        Err(AppError::DependencyInstallError(
            "macOS FFmpeg auto-download is disabled until a redistributable GPL/LGPL build is configured. Install FFmpeg with Homebrew or ship a compliant bundled ffmpeg/ffprobe sidecar.".to_string(),
        ))
    } else if cfg!(target_os = "windows") {
        Ok((
            "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
            ArchiveFormat::Zip,
        ))
    } else {
        // Linux
        if cfg!(target_arch = "aarch64") {
            Ok((
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                ArchiveFormat::TarXz,
            ))
        } else {
            Ok((
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
                ArchiveFormat::TarXz,
            ))
        }
    }
}

enum ArchiveFormat {
    Zip,
    TarXz,
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
    let (url, format) = get_download_info()?;
    let binary_names = get_binary_names();

    let temp_archive = format!(
        "ffmpeg_archive.{}",
        match format {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::TarXz => "tar.xz",
        }
    );

    // Download
    let archive_path = download_file(url, &bin_dir, &temp_archive, app, "ffmpeg").await?;

    // Extract
    emit_stage(
        app,
        "ffmpeg",
        DepInstallStage::Extracting,
        Some("Extracting ffmpeg..."),
    );

    let extracted = match format {
        ArchiveFormat::Zip => extract_zip(&archive_path, &bin_dir, binary_names).await?,
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
    // Third-party "latest" FFmpeg builds publish no stable per-release checksum, so a
    // successful `ffmpeg -version` is our integrity/sanity gate instead. If the
    // extracted binary does not run, treat the install as failed and remove the
    // broken binaries so a later dependency check does not report them as installed.
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
    if cfg!(target_os = "macos") {
        Ok("system-or-bundled".to_string())
    } else {
        // BtbN uses "latest" rolling release
        Ok("latest".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_ffmpeg_download_does_not_use_nonredistributable_builds() {
        let err = match get_download_info() {
            Ok((_url, _format)) => panic!(
                "macOS FFmpeg auto-download should be disabled until a redistributable build is configured"
            ),
            Err(err) => err,
        };

        let message = err.to_string();
        assert!(message.contains("macOS"), "{message}");
        assert!(message.contains("FFmpeg"), "{message}");
    }
}
