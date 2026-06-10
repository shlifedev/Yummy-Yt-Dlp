use super::types::{DepInstallEvent, DepInstallStage};
use crate::modules::logger;
use crate::modules::types::AppError;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// Per-dependency install/update/delete locks.
///
/// Every operation on a single dependency shares one fixed temp file name and
/// one final binary path, so two running at once would truncate each other's
/// in-flight download/extraction or race finalize-vs-delete. Serializing per
/// dependency closes that hole while still letting different dependencies (which
/// use different paths) install in parallel.
static DEP_LOCKS: std::sync::LazyLock<StdMutex<HashMap<String, Arc<AsyncMutex<()>>>>> =
    std::sync::LazyLock::new(|| StdMutex::new(HashMap::new()));

/// Acquire the lock for `dep`, serializing all filesystem-mutating work on it.
/// Hold the returned guard for the whole install/update/delete operation.
pub async fn lock_dependency(dep: &str) -> OwnedMutexGuard<()> {
    let lock = {
        let mut map = DEP_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(dep.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    };
    lock.lock_owned().await
}

/// Shared HTTP client for large archive downloads. `connect_timeout` bounds the
/// handshake and `read_timeout` bounds idle-between-bytes, so a stalled TCP
/// connection errors out instead of holding the per-dependency lock forever.
/// Deliberately no overall deadline: the archives are tens of MB and a slow but
/// progressing download must not be aborted.
static DOWNLOAD_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_default()
});

/// Shared HTTP client for small checksum/version fetches, with a short overall
/// deadline — these responses are a few KB, so 30s covers any healthy network.
static SHORT_HTTP_CLIENT: std::sync::LazyLock<reqwest::Client> = std::sync::LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default()
});

/// Client for small dependency HTTP calls (checksums, GitHub `releases/latest`).
pub(crate) fn short_http_client() -> &'static reqwest::Client {
    &SHORT_HTTP_CLIENT
}

/// Shared marker message for "install/update refused because downloads are active".
/// `dep_autoupdate` matches on it (via `is_downloads_busy_error`) to rewind its
/// throttle stamp so the next launch retries instead of waiting a full day.
pub(crate) const DOWNLOADS_BUSY_MSG: &str =
    "cannot install or update while downloads are running; retry once the queue is idle";

pub fn downloads_busy_error() -> AppError {
    AppError::DependencyInstallError(DOWNLOADS_BUSY_MSG.to_string())
}

/// Whether an install error is the downloads-busy refusal (vs a real failure).
pub(crate) fn is_downloads_busy_error(e: &AppError) -> bool {
    matches!(e, AppError::DependencyInstallError(msg) if msg == DOWNLOADS_BUSY_MSG)
}

/// True when any download is running or still queued. Binary mutation (swapping
/// yt-dlp/ffmpeg/deno) must not race a running or about-to-start download, and
/// the 300ms-delayed startup resume means `active_count` alone is not enough —
/// pending queue rows count as busy too.
pub fn downloads_busy(app: &AppHandle) -> bool {
    if let Some(manager) = app.try_state::<crate::DownloadManagerState>() {
        if manager.active_count() > 0 {
            return true;
        }
    }
    if let Some(db) = app.try_state::<crate::DbState>() {
        return db
            .get_cancellable_ids()
            .map(|ids| !ids.is_empty())
            .unwrap_or(false);
    }
    false
}

/// Remove leftover install artifacts from `bin/`: partial archive downloads,
/// staging trees from interrupted installs, and stale `.old` backups.
///
/// Must run once at app startup, before any install can begin. It must NOT run
/// inside the install path (e.g. `ensure_bin_dir`): installs for different
/// dependencies run concurrently under per-dependency locks, so a sweep there
/// could delete another dependency's in-flight temp download.
pub fn sweep_install_leftovers(app: &AppHandle) {
    let Ok(bin_dir) = ensure_bin_dir(app) else {
        return;
    };
    const LEFTOVERS: &[&str] = &[
        "yt-dlp-onedir.zip.tmp",
        "ffmpeg_archive.zip",
        "ffmpeg_archive.tar.gz",
        "ffmpeg_archive.tar.xz",
        "deno_archive.zip",
        "ytdlp.staging",
        "ytdlp.old",
        "ffmpeg.staging",
        "deno.staging",
    ];
    for name in LEFTOVERS {
        let path = bin_dir.join(name);
        if !path.exists() {
            continue;
        }
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match result {
            Ok(()) => logger::info_cat(
                "dependency",
                &format!("Removed leftover install artifact {}", name),
            ),
            Err(e) => logger::warn_cat(
                "dependency",
                &format!("Failed to remove leftover install artifact {}: {}", name, e),
            ),
        }
    }
}

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

    let response =
        DOWNLOAD_CLIENT.get(url).send().await.map_err(|e| {
            AppError::DependencyInstallError(format!("Download request failed: {}", e))
        })?;

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
    // When the server sends no Content-Length we can't compute a percentage, so emit on a
    // byte threshold instead — otherwise the UI would only ever see the single initial 0%
    // tick and look frozen for the whole download.
    let mut last_emit_bytes: u64 = 0;
    const EMIT_BYTE_STEP: u64 = 2 * 1024 * 1024; // 2 MiB
                                                 // Hard cap on how much we'll pull for a single dependency. Guards against a malicious or
                                                 // misconfigured endpoint streaming unbounded data and filling the disk.
    const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB

    use tokio::io::AsyncWriteExt;

    while let Some(chunk) = stream.next().await {
        // On every abort path, drop the handle before deleting the partial file:
        // Windows refuses to remove a file with an open handle.
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_file(&dest_path).await;
                return Err(AppError::DependencyInstallError(format!(
                    "Download stream error: {}",
                    e
                )));
            }
        };

        downloaded += chunk.len() as u64;
        if downloaded > MAX_DOWNLOAD_BYTES {
            drop(file);
            let _ = tokio::fs::remove_file(&dest_path).await;
            return Err(AppError::DependencyInstallError(format!(
                "Download exceeded the {} byte size limit; aborting",
                MAX_DOWNLOAD_BYTES
            )));
        }

        if let Err(e) = file.write_all(&chunk).await {
            drop(file);
            let _ = tokio::fs::remove_file(&dest_path).await;
            return Err(AppError::DependencyInstallError(format!(
                "Write error: {}",
                e
            )));
        }

        let (percent, should_emit) = match total_size {
            Some(total) if total > 0 => {
                let pct = (downloaded as f32 / total as f32 * 100.0).min(100.0);
                let emit = (pct - last_emit_percent).abs() >= 2.0 || downloaded >= total;
                (pct, emit)
            }
            // Unknown total: keep percent at 0.0 (the type's contract), but still emit so the
            // frontend can show byte progress and know the download is alive.
            _ => (0.0, downloaded - last_emit_bytes >= EMIT_BYTE_STEP),
        };

        if should_emit {
            last_emit_percent = percent;
            last_emit_bytes = downloaded;
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

    if let Err(e) = file.flush().await {
        drop(file);
        let _ = tokio::fs::remove_file(&dest_path).await;
        return Err(AppError::DependencyInstallError(format!(
            "Flush error: {}",
            e
        )));
    }

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
    let text = short_http_client()
        .get(url)
        .send()
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

/// Fetch a combined `<hash>  <filename>` checksum manifest (e.g. BtbN's `checksums.sha256`)
/// and return the lowercase hex hash for `filename`. Returns Ok(None) when the manifest was
/// fetched but lists no entry for the file (caller decides how strict to be).
pub async fn fetch_checksum_for(
    manifest_url: &str,
    filename: &str,
) -> Result<Option<String>, AppError> {
    let text = short_http_client()
        .get(manifest_url)
        .send()
        .await
        .map_err(|e| AppError::ChecksumError(format!("failed to fetch checksum manifest: {}", e)))?
        .error_for_status()
        .map_err(|e| AppError::ChecksumError(format!("checksum manifest unavailable: {}", e)))?
        .text()
        .await
        .map_err(|e| AppError::ChecksumError(format!("failed to read checksum manifest: {}", e)))?;

    for line in text.lines() {
        let mut parts = line.splitn(2, |c: char| c.is_whitespace());
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let name = name.trim().trim_start_matches('*');
        if name == filename {
            let hash = hash.trim().to_lowercase();
            if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(Some(hash));
            }
        }
    }
    Ok(None)
}

/// Fetch a sibling `<asset>.sha256sum` file and return the lowercase hex hash.
/// The file format is `<sha256>  <filename>`, so the leading token is the hash.
pub async fn fetch_sha256sum(url: &str) -> Result<String, AppError> {
    let text = short_http_client()
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::ChecksumError(format!("failed to fetch checksum: {}", e)))?
        .error_for_status()
        .map_err(|e| AppError::ChecksumError(format!("checksum file unavailable: {}", e)))?
        .text()
        .await
        .map_err(|e| AppError::ChecksumError(format!("failed to read checksum: {}", e)))?;

    text.split_whitespace()
        .next()
        .map(|h| h.trim().to_lowercase())
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| AppError::ChecksumError(format!("malformed checksum file at {}", url)))
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

/// Extract every entry of a zip archive into `dest_dir`, preserving the internal
/// directory structure (unlike `extract_zip`, which flattens to file names).
///
/// Used for PyInstaller `--onedir` builds, where the executable lives next to an
/// `_internal/` directory and the layout must be kept intact. Each entry path is
/// validated to stay within `dest_dir` so a crafted archive can't write outside it
/// (zip-slip). Directory entries are created; file entries get their Unix mode
/// applied so the executable bit survives.
pub async fn extract_zip_tree(archive_path: &Path, dest_dir: &Path) -> Result<(), AppError> {
    let archive = archive_path.to_path_buf();
    let dest = dest_dir.to_path_buf();

    tokio::task::spawn_blocking(move || extract_zip_tree_blocking(&archive, &dest))
        .await
        .map_err(|e| AppError::DependencyInstallError(format!("Extract task failed: {}", e)))?
}

fn extract_zip_tree_blocking(archive: &Path, dest: &Path) -> Result<(), AppError> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::DependencyInstallError(format!("Failed to open zip: {}", e)))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::DependencyInstallError(format!("Failed to read zip: {}", e)))?;

    std::fs::create_dir_all(dest).map_err(|e| {
        AppError::DependencyInstallError(format!("Failed to create extract dir: {}", e))
    })?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::DependencyInstallError(format!("Zip entry error: {}", e)))?;

        // `enclosed_name` returns None for absolute paths or any `..` component
        // that would escape the destination — exactly the zip-slip cases we reject.
        let rel = entry.enclosed_name().ok_or_else(|| {
            AppError::DependencyInstallError(format!(
                "Refusing unsafe zip entry path: {}",
                entry.name()
            ))
        })?;
        let out_path = dest.join(&rel);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| {
                AppError::DependencyInstallError(format!("Failed to create dir: {}", e))
            })?;
            continue;
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::DependencyInstallError(format!("Failed to create parent dir: {}", e))
            })?;
        }
        let mut out_file = std::fs::File::create(&out_path).map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to create extracted file: {}", e))
        })?;
        std::io::copy(&mut entry, &mut out_file).map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to extract file: {}", e))
        })?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
        }
    }

    Ok(())
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

/// Strip the macOS quarantine attribute recursively from a directory tree.
///
/// onedir yt-dlp ships ~20 files under `_internal/`; each one inherits the bundle's
/// quarantine, and Gatekeeper re-checks any quarantined dylib the executable loads.
/// `xattr -r` clears the whole tree in one call.
#[cfg(target_os = "macos")]
pub fn remove_quarantine_recursive(dir: &Path) -> Result<(), AppError> {
    let _ = std::process::Command::new("xattr")
        .args(["-r", "-d", "com.apple.quarantine"])
        .arg(dir)
        .output();
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn remove_quarantine_recursive(_dir: &Path) -> Result<(), AppError> {
    Ok(())
}

/// Recursively copy `src` directory into `dest` (creating `dest`), preserving Unix
/// permissions so an executable bit on the inner binary survives the copy.
pub fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(dest).map_err(|e| {
        AppError::DependencyInstallError(format!("Failed to create dir {}: {}", dest.display(), e))
    })?;
    for entry in std::fs::read_dir(src).map_err(|e| {
        AppError::DependencyInstallError(format!("Failed to read dir {}: {}", src.display(), e))
    })? {
        let entry = entry.map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to read dir entry: {}", e))
        })?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let file_type = entry.file_type().map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to stat dir entry: {}", e))
        })?;
        if file_type.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| {
                AppError::DependencyInstallError(format!(
                    "Failed to copy {}: {}",
                    from.display(),
                    e
                ))
            })?;
        }
    }
    Ok(())
}

/// Atomically replace the directory at `final_dir` with `staging_dir`.
///
/// Renames the old directory aside first so an in-flight process keeps its files,
/// then moves the freshly staged tree into place. On failure the old copy is
/// restored; on success the old copy is removed best-effort.
pub fn finalize_dir(staging_dir: &Path, final_dir: &Path) -> Result<(), AppError> {
    if !final_dir.exists() {
        if let Some(parent) = final_dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::DependencyInstallError(format!("Failed to create parent dir: {}", e))
            })?;
        }
        return std::fs::rename(staging_dir, final_dir).map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to install dir: {}", e))
        });
    }

    let backup = final_dir.with_extension("old");
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(final_dir, &backup).map_err(|e| {
        AppError::DependencyInstallError(format!("Failed to move aside old dir: {}", e))
    })?;

    match std::fs::rename(staging_dir, final_dir) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(e) => {
            // Put the old copy back so the install isn't left empty.
            let _ = std::fs::rename(&backup, final_dir);
            Err(AppError::DependencyInstallError(format!(
                "Failed to swap in new dir: {}",
                e
            )))
        }
    }
}

/// Why a binary version probe failed — lets callers distinguish a slow cold
/// start (timeout) from a binary that cannot run at all (spawn error, non-zero
/// exit, empty output).
#[derive(Debug)]
pub enum VersionProbeError {
    Timeout(u64),
    Spawn(String),
    Exit { status: String, stderr: String },
    EmptyOutput,
}

impl std::fmt::Display for VersionProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout(secs) => write!(f, "timed out after {}s", secs),
            Self::Spawn(e) => write!(f, "failed to start: {}", e),
            Self::Exit { status, stderr } => {
                write!(f, "exited with {}; stderr: {}", status, stderr)
            }
            Self::EmptyOutput => write!(f, "produced no version output"),
        }
    }
}

/// Run a binary with its version flag and return the first output line.
pub async fn probe_binary_version(
    binary_path: &Path,
    version_flag: &str,
    timeout: Duration,
) -> Result<String, VersionProbeError> {
    let mut cmd = tokio::process::Command::new(binary_path);
    cmd.arg(version_flag);
    // A probe that outlives its timeout must not linger as an orphan.
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| VersionProbeError::Timeout(timeout.as_secs()))?
        .map_err(|e| VersionProbeError::Spawn(format!("{} ({})", e, e.kind())))?;

    if !output.status.success() {
        return Err(VersionProbeError::Exit {
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or(VersionProbeError::EmptyOutput)
}

/// Get the version string from a binary by running it with a version flag.
pub async fn get_binary_version(binary_path: &Path, version_flag: &str) -> Option<String> {
    // yt-dlp's onedir build does a one-time PyInstaller bootstrap on its first run
    // (the post-install verification call), measured around 9s on macOS — keep the
    // timeout well above that so a slow cold start isn't misread as a failed install.
    probe_binary_version(binary_path, version_flag, Duration::from_secs(25))
        .await
        .ok()
}

/// Verify a freshly staged binary before swapping it into place. Retries once on
/// timeout (cold or loaded machines), but treats spawn errors, non-zero exits,
/// persistent timeouts, and empty output as a hard failure — an unverifiable
/// staged copy must never replace a working one.
pub async fn verify_staged_binary(path: &Path, version_flag: &str) -> Result<String, AppError> {
    let mut probe = probe_binary_version(path, version_flag, Duration::from_secs(20)).await;
    if matches!(probe, Err(VersionProbeError::Timeout(_))) {
        probe = probe_binary_version(path, version_flag, Duration::from_secs(40)).await;
    }
    probe.map_err(|e| {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        AppError::DependencyInstallError(format!(
            "{} installed but failed to run ({}): {}; the download may be corrupt or incompatible",
            name, version_flag, e
        ))
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn unique_dir(tag: &str) -> PathBuf {
        let nanos = SystemTimeExt::nanos();
        std::env::temp_dir().join(format!(
            "dep_dl_test_{}_{}_{}",
            tag,
            std::process::id(),
            nanos
        ))
    }

    // Tiny monotonic-ish suffix so parallel tests don't collide on the temp dir.
    struct SystemTimeExt;
    impl SystemTimeExt {
        fn nanos() -> u128 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, data) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extract_zip_tree_preserves_structure() {
        let work = unique_dir("ok");
        std::fs::create_dir_all(&work).unwrap();
        let archive = work.join("a.zip");
        write_zip(
            &archive,
            &[
                ("yt-dlp_macos", b"#!bin"),
                ("_internal/base_library.zip", b"lib"),
                ("_internal/sub/mod.so", b"so"),
            ],
        );

        let dest = work.join("out");
        extract_zip_tree_blocking(&archive, &dest).unwrap();

        assert!(dest.join("yt-dlp_macos").is_file());
        assert!(dest.join("_internal/base_library.zip").is_file());
        assert!(dest.join("_internal/sub/mod.so").is_file());

        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn extract_zip_tree_rejects_zip_slip() {
        let work = unique_dir("slip");
        std::fs::create_dir_all(&work).unwrap();
        let archive = work.join("evil.zip");
        // A traversal entry that would land outside the destination dir.
        write_zip(&archive, &[("../../escape.txt", b"pwned")]);

        let dest = work.join("out");
        let result = extract_zip_tree_blocking(&archive, &dest);
        assert!(result.is_err(), "zip-slip entry must be rejected");

        // The escape target must not have been written.
        assert!(!work.join("escape.txt").exists());
        assert!(!dest.join("escape.txt").exists());

        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn finalize_dir_replaces_existing_tree() {
        let work = unique_dir("finalize");
        std::fs::create_dir_all(&work).unwrap();

        let final_dir = work.join("ytdlp");
        std::fs::create_dir_all(&final_dir).unwrap();
        std::fs::write(final_dir.join("old.txt"), b"old").unwrap();

        let staging = work.join("staging");
        std::fs::create_dir_all(staging.join("_internal")).unwrap();
        std::fs::write(staging.join("yt-dlp"), b"new").unwrap();

        finalize_dir(&staging, &final_dir).unwrap();

        assert!(final_dir.join("yt-dlp").is_file());
        assert!(final_dir.join("_internal").is_dir());
        // Old contents are gone after the swap.
        assert!(!final_dir.join("old.txt").exists());
        // Staging was consumed by the rename.
        assert!(!staging.exists());

        let _ = std::fs::remove_dir_all(&work);
    }
}
