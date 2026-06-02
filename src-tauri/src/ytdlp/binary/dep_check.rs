use super::path::{app_bin_dir, command_with_path, dep_mode, DepMode};
use super::resolve::{check_deno_version, check_ffmpeg, check_ytdlp, try_get_version};
use crate::ytdlp::types::{DepInfo, DepSource, FullDependencyStatus};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

/// Cached dependency status with TTL.
struct DepStatusCache {
    status: FullDependencyStatus,
    cached_at: Instant,
}

static DEP_CACHE: std::sync::LazyLock<RwLock<Option<DepStatusCache>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// Invalidate the dependency status cache.
/// Called after install/delete/update operations or dep_mode changes.
pub fn invalidate_dep_cache() {
    if let Ok(mut guard) = DEP_CACHE.write() {
        *guard = None;
    }
}

/// Quick check if a binary exists on the augmented PATH using which/where.
/// Much faster than spawning the binary with --version.
async fn quick_binary_exists(name: &str) -> bool {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut cmd = command_with_path(which_cmd);
    cmd.arg(name);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    matches!(
        tokio::time::timeout(Duration::from_secs(3), cmd.output()).await,
        Ok(Ok(output)) if output.status.success()
    )
}

/// Resolve a binary's absolute path via which/where (first match), for display.
async fn which_path(name: &str) -> Option<String> {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut cmd = command_with_path(which_cmd);
    cmd.arg(name);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let output = tokio::time::timeout(Duration::from_secs(3), cmd.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn not_found() -> DepInfo {
    DepInfo {
        installed: false,
        version: None,
        source: DepSource::NotFound,
        path: None,
    }
}

/// app-managed yt-dlp, if the binary exists in the app bin dir.
async fn app_managed_ytdlp(app: &AppHandle) -> Option<DepInfo> {
    let bin_name = if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    let app_binary = app_bin_dir(app)?.join(bin_name);
    if !app_binary.exists() {
        return None;
    }
    // Binary file exists in app bin dir — report as installed.
    // Version check may fail on first run (PyInstaller extraction, Gatekeeper, etc.)
    let version = try_get_version(&app_binary).await.ok();
    Some(DepInfo {
        installed: true,
        version,
        source: DepSource::AppManaged,
        path: Some(app_binary.to_string_lossy().to_string()),
    })
}

/// system-PATH yt-dlp, if discoverable.
async fn system_ytdlp() -> Option<DepInfo> {
    // Quick existence check via which/where before spawning yt-dlp --version
    if !quick_binary_exists("yt-dlp").await {
        return None;
    }
    let (version, _debug) = check_ytdlp().await;
    version.map(|ver| DepInfo {
        installed: true,
        version: Some(ver),
        source: DepSource::SystemPath,
        path: None, // filled in by caller via which_path
    })
}

async fn check_dep_ytdlp(app: &AppHandle) -> DepInfo {
    let info = match dep_mode(app) {
        DepMode::Bundled => match app_managed_ytdlp(app).await {
            Some(info) => Some(info),
            None => system_ytdlp().await,
        },
        DepMode::Hybrid => match system_ytdlp().await {
            Some(info) => Some(info),
            None => app_managed_ytdlp(app).await,
        },
    };

    match info {
        Some(mut info) => {
            // Resolve the display path lazily for system-sourced binaries.
            if info.source == DepSource::SystemPath && info.path.is_none() {
                info.path = which_path("yt-dlp").await;
            }
            info
        }
        None => not_found(),
    }
}

/// app-managed ffmpeg, if the binary exists in the app bin dir.
async fn app_managed_ffmpeg(app: &AppHandle) -> Option<DepInfo> {
    let bin_name = if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let app_binary = app_bin_dir(app)?.join(bin_name);
    if !app_binary.exists() {
        return None;
    }

    let mut version: Option<String> = None;
    let mut cmd = tokio::process::Command::new(&app_binary);
    cmd.arg("-version");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(5), cmd.output()).await {
        if output.status.success() {
            version = Some(
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string(),
            );
        }
    }
    Some(DepInfo {
        installed: true,
        version,
        source: DepSource::AppManaged,
        path: Some(app_binary.to_string_lossy().to_string()),
    })
}

/// system-PATH ffmpeg, if discoverable.
async fn system_ffmpeg() -> Option<DepInfo> {
    check_ffmpeg().await.map(|version| DepInfo {
        installed: true,
        version: Some(version),
        source: DepSource::SystemPath,
        path: None, // filled in by caller via which_path
    })
}

async fn check_dep_ffmpeg(app: &AppHandle) -> DepInfo {
    let info = match dep_mode(app) {
        DepMode::Bundled => match app_managed_ffmpeg(app).await {
            Some(info) => Some(info),
            None => system_ffmpeg().await,
        },
        DepMode::Hybrid => match system_ffmpeg().await {
            Some(info) => Some(info),
            None => app_managed_ffmpeg(app).await,
        },
    };

    match info {
        Some(mut info) => {
            if info.source == DepSource::SystemPath && info.path.is_none() {
                info.path = which_path("ffmpeg").await;
            }
            info
        }
        None => not_found(),
    }
}

async fn check_dep_deno(app: &AppHandle) -> DepInfo {
    if let Some(deno_path) = super::resolve::resolve_deno_path(app).await {
        let is_app_managed = app_bin_dir(app)
            .map(|bin_dir| deno_path.starts_with(bin_dir))
            .unwrap_or(false);

        let version = check_deno_version(&deno_path).await;

        DepInfo {
            installed: true,
            version,
            source: if is_app_managed {
                DepSource::AppManaged
            } else {
                DepSource::SystemPath
            },
            path: Some(deno_path.to_string_lossy().to_string()),
        }
    } else {
        DepInfo {
            installed: false,
            version: None,
            source: DepSource::NotFound,
            path: None,
        }
    }
}

const DEP_CACHE_STORE: &str = "dep-cache.json";

/// Save dependency status to persistent store for instant load on next app launch.
fn save_dep_status_to_store(app: &AppHandle, status: &FullDependencyStatus) {
    if let Ok(store) = app.store(DEP_CACHE_STORE) {
        if let Ok(val) = serde_json::to_value(status) {
            store.set("depStatus", val);
            let _ = store.save();
        }
    }
}

/// Load cached dependency status from persistent store.
/// Returns the previously saved FullDependencyStatus if available.
pub fn get_cached_dep_status(app: &AppHandle) -> Option<FullDependencyStatus> {
    let store = app.store(DEP_CACHE_STORE).ok()?;
    let val = store.get("depStatus")?;
    serde_json::from_value(val).ok()
}

/// Get full dependency status including yt-dlp, ffmpeg, and deno.
/// Uses a 60-second cache to avoid repeated subprocess spawns on page navigation.
pub async fn check_full_dependencies(app: &AppHandle) -> FullDependencyStatus {
    // Check cache (60s TTL)
    if let Ok(guard) = DEP_CACHE.read() {
        if let Some(cached) = guard.as_ref() {
            if cached.cached_at.elapsed() < Duration::from_secs(60) {
                return cached.status.clone();
            }
        }
    }

    let (ytdlp_info, ffmpeg_info, deno_info) = tokio::join!(
        check_dep_ytdlp(app),
        check_dep_ffmpeg(app),
        check_dep_deno(app),
    );
    let result = FullDependencyStatus {
        ytdlp: ytdlp_info,
        ffmpeg: ffmpeg_info,
        deno: deno_info,
    };

    // Store in memory cache
    if let Ok(mut guard) = DEP_CACHE.write() {
        *guard = Some(DepStatusCache {
            status: result.clone(),
            cached_at: Instant::now(),
        });
    }

    // Persist to store for instant load on next app launch
    save_dep_status_to_store(app, &result);

    result
}

/// Warmup yt-dlp by running `--version` in the background.
/// PyInstaller `--onefile` binaries need to extract the Python runtime on each run;
/// triggering this early primes the OS file cache so subsequent invocations are faster.
pub fn warmup_ytdlp(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let path = match super::resolve::resolve_ytdlp_path_with_app(&app).await {
            Ok(p) => p,
            Err(_) => return,
        };
        let mut cmd = super::path::command_with_path_app(&path, &app);
        cmd.arg("--version");

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // Result is intentionally ignored; this is purely for OS file cache priming.
        let _ = tokio::time::timeout(Duration::from_secs(30), cmd.output()).await;
    });
}
