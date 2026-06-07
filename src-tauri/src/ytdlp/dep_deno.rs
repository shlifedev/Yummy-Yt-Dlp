use super::dep_download::*;
use super::types::DepInstallStage;
use crate::modules::types::AppError;
use tauri::AppHandle;

/// Get deno download URL for the current platform.
fn get_download_url() -> Result<&'static str, AppError> {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            Ok("https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip")
        } else {
            Ok("https://github.com/denoland/deno/releases/latest/download/deno-x86_64-apple-darwin.zip")
        }
    } else if cfg!(target_os = "windows") {
        Ok("https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip")
    } else {
        // Linux
        if cfg!(target_arch = "aarch64") {
            Ok("https://github.com/denoland/deno/releases/latest/download/deno-aarch64-unknown-linux-gnu.zip")
        } else {
            Ok("https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip")
        }
    }
}

/// Get deno binary name for the current platform.
fn get_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "deno.exe"
    } else {
        "deno"
    }
}

/// Install deno by downloading from GitHub releases.
pub async fn install_deno(app: &AppHandle) -> Result<String, AppError> {
    // Serialize against any concurrent deno install/update/delete so they don't
    // corrupt the shared archive temp file or race the extracted binary.
    let _lock = lock_dependency("deno").await;
    let bin_dir = ensure_bin_dir(app)?;
    let url = get_download_url()?;
    let binary_name = get_binary_name();

    // Download zip
    let archive_path = download_file(url, &bin_dir, "deno_archive.zip", app, "deno").await?;

    // Verify against deno's published per-asset checksum (<asset>.zip.sha256sum)
    // before extracting/executing. Fail closed: a zip we cannot verify is removed.
    emit_stage(
        app,
        "deno",
        DepInstallStage::Verifying,
        Some("Verifying checksum..."),
    );
    let expected = match fetch_sha256sum(&format!("{}.sha256sum", url)).await {
        Ok(h) => h,
        Err(e) => {
            let _ = tokio::fs::remove_file(&archive_path).await;
            return Err(e);
        }
    };
    if let Err(e) = verify_sha256(&archive_path, &expected).await {
        let _ = tokio::fs::remove_file(&archive_path).await;
        return Err(e);
    }

    // Extract (deno binary is at zip root)
    emit_stage(
        app,
        "deno",
        DepInstallStage::Extracting,
        Some("Extracting deno..."),
    );
    let extracted = extract_zip(&archive_path, &bin_dir, &[binary_name]).await?;

    if extracted.is_empty() {
        let _ = tokio::fs::remove_file(&archive_path).await;
        return Err(AppError::DependencyInstallError(
            "deno binary not found in archive".to_string(),
        ));
    }

    // Set executable + remove quarantine
    for path in &extracted {
        set_executable(path)?;
        remove_quarantine(path)?;
    }

    // Clean up archive
    let _ = tokio::fs::remove_file(&archive_path).await;

    // Verify installation
    emit_stage(
        app,
        "deno",
        DepInstallStage::Completing,
        Some("Verifying installation..."),
    );
    let deno_path = bin_dir.join(binary_name);
    let version = get_binary_version(&deno_path, "--version")
        .await
        .unwrap_or_else(|| "unknown".to_string());

    emit_stage(
        app,
        "deno",
        DepInstallStage::Completing,
        Some("deno installed"),
    );

    Ok(version)
}

/// Get the latest deno version from GitHub API.
pub async fn get_latest_version() -> Result<String, AppError> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/repos/denoland/deno/releases/latest")
        .header("User-Agent", "modern-ytdlp-gui")
        .send()
        .await
        .map_err(|e| AppError::NetworkError(format!("Failed to check deno version: {}", e)))?;

    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| AppError::NetworkError(format!("Failed to parse response: {}", e)))?;

    json["tag_name"]
        .as_str()
        .map(|s: &str| s.to_string())
        .ok_or_else(|| AppError::NetworkError("No tag_name in response".to_string()))
}
