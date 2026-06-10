use crate::modules::logger;
use crate::modules::types::AppError;
use crate::ytdlp::binary;
use crate::ytdlp::types::*;
use tauri::AppHandle;

#[tauri::command]
#[specta::specta]
pub async fn check_dependencies() -> Result<DependencyStatus, AppError> {
    Ok(binary::check_dependencies().await)
}

#[tauri::command]
#[specta::specta]
pub async fn update_ytdlp(app: AppHandle) -> Result<String, AppError> {
    binary::update_ytdlp(&app).await
}

#[tauri::command]
#[specta::specta]
pub fn get_cached_dep_status(app: AppHandle) -> Result<Option<FullDependencyStatus>, AppError> {
    Ok(binary::get_cached_dep_status(&app))
}

#[tauri::command]
#[specta::specta]
pub async fn check_full_dependencies(
    app: AppHandle,
    force: Option<bool>,
) -> Result<FullDependencyStatus, AppError> {
    if force.unwrap_or(false) {
        binary::invalidate_dep_cache();
    }
    let status = binary::check_full_dependencies(&app).await;
    logger::info_cat(
        "dependency",
        &format!(
            "Dependency check: yt-dlp={}, ffmpeg={}, deno={}",
            status.ytdlp.installed, status.ffmpeg.installed, status.deno.installed
        ),
    );
    Ok(status)
}

#[tauri::command]
#[specta::specta]
pub async fn install_dependency(app: AppHandle, dep_name: String) -> Result<String, AppError> {
    let result = match dep_name.as_str() {
        "yt-dlp" => crate::ytdlp::dep_ytdlp::install_ytdlp(&app).await,
        "ffmpeg" => crate::ytdlp::dep_ffmpeg::install_ffmpeg(&app).await,
        "deno" => crate::ytdlp::dep_deno::install_deno(&app).await,
        _ => Err(AppError::DependencyInstallError(format!(
            "Unknown dependency: {}",
            dep_name
        ))),
    };
    binary::invalidate_dep_cache();
    result
}

#[tauri::command]
#[specta::specta]
pub async fn install_all_dependencies(app: AppHandle) -> Result<Vec<String>, AppError> {
    let status = binary::check_full_dependencies(&app).await;
    let mut results = Vec::new();

    // Run all three installations concurrently with tokio::join!
    let (ytdlp_res, ffmpeg_res, deno_res) = tokio::join!(
        async {
            if !status.ytdlp.installed {
                Some(crate::ytdlp::dep_ytdlp::install_ytdlp(&app).await)
            } else {
                None
            }
        },
        async {
            if !status.ffmpeg.installed {
                Some(crate::ytdlp::dep_ffmpeg::install_ffmpeg(&app).await)
            } else {
                None
            }
        },
        async {
            if !status.deno.installed {
                Some(crate::ytdlp::dep_deno::install_deno(&app).await)
            } else {
                None
            }
        },
    );

    if let Some(res) = ytdlp_res {
        match res {
            Ok(v) => results.push(format!("yt-dlp: {}", v)),
            Err(e) => {
                crate::ytdlp::dep_download::emit_stage(
                    &app,
                    "yt-dlp",
                    DepInstallStage::Failed,
                    Some(&e.to_string()),
                );
                results.push(format!("yt-dlp: FAILED - {}", e));
            }
        }
    }

    if let Some(res) = ffmpeg_res {
        match res {
            Ok(v) => results.push(format!("ffmpeg: {}", v)),
            Err(e) => {
                crate::ytdlp::dep_download::emit_stage(
                    &app,
                    "ffmpeg",
                    DepInstallStage::Failed,
                    Some(&e.to_string()),
                );
                results.push(format!("ffmpeg: FAILED - {}", e));
            }
        }
    }

    if let Some(res) = deno_res {
        match res {
            Ok(v) => results.push(format!("deno: {}", v)),
            Err(e) => {
                crate::ytdlp::dep_download::emit_stage(
                    &app,
                    "deno",
                    DepInstallStage::Failed,
                    Some(&e.to_string()),
                );
                results.push(format!("deno: FAILED - {}", e));
            }
        }
    }

    binary::invalidate_dep_cache();
    Ok(results)
}

#[tauri::command]
#[specta::specta]
pub async fn check_dependency_update(
    app: AppHandle,
    dep_name: String,
) -> Result<DepUpdateInfo, AppError> {
    let latest = match dep_name.as_str() {
        "yt-dlp" => crate::ytdlp::dep_ytdlp::get_latest_version().await?,
        "ffmpeg" => crate::ytdlp::dep_ffmpeg::get_latest_version().await?,
        "deno" => crate::ytdlp::dep_deno::get_latest_version().await?,
        _ => {
            return Err(AppError::DependencyInstallError(format!(
                "Unknown dependency: {}",
                dep_name
            )))
        }
    };

    // Fetch current installed version from cached dependency status
    let current_version =
        binary::get_cached_dep_status(&app).and_then(|status| match dep_name.as_str() {
            "yt-dlp" => status.ytdlp.version,
            "ffmpeg" => status.ffmpeg.version,
            "deno" => status.deno.version,
            _ => None,
        });

    // Compare versions: update is available if we can't determine current version
    // or if the latest version string differs from the current one
    let update_available = match &current_version {
        Some(current) => {
            // Normalize: strip leading 'v' for comparison
            let current_normalized = current.trim_start_matches('v');
            let latest_normalized = latest.trim_start_matches('v');
            current_normalized != latest_normalized
        }
        None => true,
    };

    Ok(DepUpdateInfo {
        current_version,
        latest_version: latest,
        update_available,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn update_dependency(app: AppHandle, dep_name: String) -> Result<String, AppError> {
    // Re-install (download latest) is the update mechanism
    install_dependency(app, dep_name).await
}

/// Delete an app-managed dependency binary from app_data_dir/bin/.
#[tauri::command]
#[specta::specta]
pub async fn delete_app_managed_dep(app: AppHandle, dep_name: String) -> Result<String, AppError> {
    let bin_dir = crate::ytdlp::dep_download::ensure_bin_dir(&app)?;

    // Each list also covers install leftovers (staging trees, stale backups,
    // partial archives) so "Delete Binary" really clears everything on disk.
    let names_to_delete: Vec<&str> = match dep_name.as_str() {
        "yt-dlp" => {
            // The onedir tree lives under bin/ytdlp/; the bare file is only present
            // for legacy installs. Try both so a delete fully clears yt-dlp.
            if cfg!(target_os = "windows") {
                vec![
                    "ytdlp",
                    "ytdlp.old",
                    "ytdlp.staging",
                    "yt-dlp-onedir.zip.tmp",
                    "yt-dlp.exe",
                ]
            } else {
                vec![
                    "ytdlp",
                    "ytdlp.old",
                    "ytdlp.staging",
                    "yt-dlp-onedir.zip.tmp",
                    "yt-dlp",
                ]
            }
        }
        "ffmpeg" => {
            if cfg!(target_os = "windows") {
                vec![
                    "ffmpeg.exe",
                    "ffprobe.exe",
                    "ffmpeg.staging",
                    "ffmpeg_archive.zip",
                    "ffmpeg_archive.tar.gz",
                    "ffmpeg_archive.tar.xz",
                ]
            } else {
                vec![
                    "ffmpeg",
                    "ffprobe",
                    "ffmpeg.staging",
                    "ffmpeg_archive.zip",
                    "ffmpeg_archive.tar.gz",
                    "ffmpeg_archive.tar.xz",
                ]
            }
        }
        "deno" => {
            if cfg!(target_os = "windows") {
                vec!["deno.exe", "deno.staging", "deno_archive.zip"]
            } else {
                vec!["deno", "deno.staging", "deno_archive.zip"]
            }
        }
        _ => {
            return Err(AppError::DependencyInstallError(format!(
                "Unknown dependency: {}",
                dep_name
            )))
        }
    };

    // Hold the same per-dependency lock the installers use, so a delete can't run
    // halfway through an in-flight install/update of this dependency.
    let _lock = crate::ytdlp::dep_download::lock_dependency(&dep_name).await;

    let mut deleted = Vec::new();
    for name in &names_to_delete {
        let path = bin_dir.join(name);
        if !path.exists() {
            continue;
        }
        let result = if path.is_dir() {
            tokio::fs::remove_dir_all(&path).await
        } else {
            tokio::fs::remove_file(&path).await
        };
        result.map_err(|e| {
            AppError::DependencyInstallError(format!("Failed to delete {}: {}", name, e))
        })?;
        deleted.push(name.to_string());
    }

    binary::invalidate_dep_cache();
    if deleted.is_empty() {
        Ok(format!("{}: no app-managed binaries found", dep_name))
    } else {
        Ok(format!("{}: deleted {}", dep_name, deleted.join(", ")))
    }
}
