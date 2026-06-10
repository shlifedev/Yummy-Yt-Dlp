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

    // Extract into a staging directory (deno binary is at the zip root), never
    // directly over the live binary: File::create would truncate a working deno,
    // so an interrupted extraction must land in a disposable location.
    emit_stage(
        app,
        "deno",
        DepInstallStage::Extracting,
        Some("Extracting deno..."),
    );
    let staging = bin_dir.join("deno.staging");
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = std::fs::create_dir_all(&staging) {
        let _ = tokio::fs::remove_file(&archive_path).await;
        return Err(AppError::DependencyInstallError(format!(
            "Failed to create staging dir: {}",
            e
        )));
    }
    let extract_result = extract_zip(&archive_path, &staging, &[binary_name]).await;
    // The archive is no longer needed whether or not extraction worked.
    let _ = tokio::fs::remove_file(&archive_path).await;
    let extracted = match extract_result {
        Ok(files) => files,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    let staged_deno = staging.join(binary_name);
    if extracted.is_empty() || !staged_deno.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(AppError::DependencyInstallError(
            "deno binary not found in archive".to_string(),
        ));
    }

    // Executable bit + quarantine strip happen on the STAGED file, before the
    // version probe (which needs to run it) and the swap.
    if let Err(e) = set_executable(&staged_deno) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let _ = remove_quarantine(&staged_deno);

    // Verify the STAGED binary before swapping: a binary that cannot run must
    // never report a successful install (or replace a working copy).
    emit_stage(
        app,
        "deno",
        DepInstallStage::Verifying,
        Some("Verifying installation..."),
    );
    let version = match verify_staged_binary(&staged_deno, "--version").await {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            emit_stage(app, "deno", DepInstallStage::Failed, Some(&e.to_string()));
            return Err(e);
        }
    };

    // Last-moment gate under the dependency lock: never swap the binary while a
    // download could be running it (yt-dlp spawns deno for some extractors).
    if downloads_busy(app) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(downloads_busy_error());
    }

    // `std::fs::rename` replaces an existing destination; if the target is locked
    // by a running process the rename fails and the old binary is left untouched.
    if let Err(e) = std::fs::rename(&staged_deno, bin_dir.join(binary_name)) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(AppError::DependencyInstallError(format!(
            "Failed to install deno: {}",
            e
        )));
    }
    let _ = std::fs::remove_dir_all(&staging);

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
    let resp = short_http_client()
        .get("https://api.github.com/repos/denoland/deno/releases/latest")
        .header("User-Agent", "yummy-yt-dlp")
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
