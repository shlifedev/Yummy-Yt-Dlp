use super::path::{command_with_path, source_order, DepSourcePref};
use super::resolve::{
    app_managed_binary, check_deno_version, check_ffmpeg, check_ytdlp, deno_home_path,
    deno_on_system_path, try_get_version, which_first,
};
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
        cmd.creation_flags(0x08000000);
    }

    matches!(
        tokio::time::timeout(Duration::from_secs(3), cmd.output()).await,
        Ok(Ok(output)) if output.status.success()
    )
}

fn not_found() -> DepInfo {
    DepInfo {
        installed: false,
        version: None,
        source: DepSource::NotFound,
        path: None,
        app_available: false,
        system_available: false,
    }
}

/// Pick the active variant from the app-managed and system candidates following
/// the effective source order for `dep`, while recording which sources exist so
/// the UI can offer a per-item toggle.
fn choose_dep(
    app: &AppHandle,
    dep: &str,
    app_info: Option<DepInfo>,
    system_info: Option<DepInfo>,
) -> DepInfo {
    let app_available = app_info.is_some();
    let system_available = system_info.is_some();

    let mut active: Option<DepInfo> = None;
    for src in source_order(app, dep) {
        match src {
            DepSourcePref::AppManaged => {
                if let Some(info) = &app_info {
                    active = Some(info.clone());
                    break;
                }
            }
            DepSourcePref::SystemPath => {
                if let Some(info) = &system_info {
                    active = Some(info.clone());
                    break;
                }
            }
        }
    }

    let mut info = active.unwrap_or_else(not_found);
    info.app_available = app_available;
    info.system_available = system_available;
    info
}

/// app-managed yt-dlp, if the binary exists in the app bin dir.
async fn app_managed_ytdlp(app: &AppHandle) -> Option<DepInfo> {
    let app_binary = app_managed_binary(app, "yt-dlp", "yt-dlp.exe")?;
    // Version check may fail on first run (PyInstaller extraction, Gatekeeper, etc.),
    // but the binary's presence on disk is enough to report it installed.
    let version = try_get_version(&app_binary).await.ok();
    Some(DepInfo {
        installed: true,
        version,
        source: DepSource::AppManaged,
        path: Some(app_binary.to_string_lossy().to_string()),
        app_available: true,
        system_available: false,
    })
}

/// system-PATH yt-dlp, if discoverable.
async fn system_ytdlp() -> Option<DepInfo> {
    // Quick existence check via which/where before spawning yt-dlp --version
    if !quick_binary_exists("yt-dlp").await {
        return None;
    }
    let (version, _debug) = check_ytdlp().await;
    let version = version?;
    Some(DepInfo {
        installed: true,
        version: Some(version),
        source: DepSource::SystemPath,
        path: which_first("yt-dlp").await,
        app_available: false,
        system_available: true,
    })
}

async fn check_dep_ytdlp(app: &AppHandle) -> DepInfo {
    let (app_info, system_info) = tokio::join!(app_managed_ytdlp(app), system_ytdlp());
    choose_dep(app, "yt-dlp", app_info, system_info)
}

/// app-managed ffmpeg, if the binary exists in the app bin dir.
async fn app_managed_ffmpeg(app: &AppHandle) -> Option<DepInfo> {
    let app_binary = app_managed_binary(app, "ffmpeg", "ffmpeg.exe")?;

    let mut version: Option<String> = None;
    let mut cmd = tokio::process::Command::new(&app_binary);
    cmd.arg("-version");
    #[cfg(target_os = "windows")]
    {
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
        app_available: true,
        system_available: false,
    })
}

/// system-PATH ffmpeg, if discoverable.
async fn system_ffmpeg() -> Option<DepInfo> {
    let version = check_ffmpeg().await?;
    Some(DepInfo {
        installed: true,
        version: Some(version),
        source: DepSource::SystemPath,
        path: which_first("ffmpeg").await,
        app_available: false,
        system_available: true,
    })
}

async fn check_dep_ffmpeg(app: &AppHandle) -> DepInfo {
    let (app_info, system_info) = tokio::join!(app_managed_ffmpeg(app), system_ffmpeg());
    choose_dep(app, "ffmpeg", app_info, system_info)
}

/// app-managed deno, if the binary exists in the app bin dir.
async fn app_managed_deno(app: &AppHandle) -> Option<DepInfo> {
    let app_binary = app_managed_binary(app, "deno", "deno.exe")?;
    let version = check_deno_version(&app_binary).await;
    Some(DepInfo {
        installed: true,
        version,
        source: DepSource::AppManaged,
        path: Some(app_binary.to_string_lossy().to_string()),
        app_available: true,
        system_available: false,
    })
}

/// system deno (default `~/.deno/bin` first, then PATH), if discoverable.
async fn system_deno() -> Option<DepInfo> {
    let path = match deno_home_path() {
        Some(p) => p,
        None => deno_on_system_path().await?,
    };
    let version = check_deno_version(&path).await;
    Some(DepInfo {
        installed: true,
        version,
        source: DepSource::SystemPath,
        path: Some(path.to_string_lossy().to_string()),
        app_available: false,
        system_available: true,
    })
}

async fn check_dep_deno(app: &AppHandle) -> DepInfo {
    let (app_info, system_info) = tokio::join!(app_managed_deno(app), system_deno());
    choose_dep(app, "deno", app_info, system_info)
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
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // Result is intentionally ignored; this is purely for OS file cache priming.
        let _ = tokio::time::timeout(Duration::from_secs(30), cmd.output()).await;
    });
}
