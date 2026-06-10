//! Keep app-managed bundled binaries fresh on launch.
//!
//! The seed step (`dep_seed`) only copies a bundled binary into `app_data/bin`
//! once and never overwrites it, and the bundle itself is frozen at the app's
//! release-build time. For a yt-dlp GUI that's a problem: yt-dlp ships every few
//! weeks and YouTube breaks extractors constantly, so a stale yt-dlp silently
//! stops downloading. This module runs a throttled background check on startup
//! and re-downloads the latest when needed, reusing the existing install flow.

use crate::modules::logger;
use crate::ytdlp::binary::invalidate_dep_cache;
use crate::ytdlp::dep_download::{
    downloads_busy, ensure_bin_dir, get_binary_version, is_downloads_busy_error,
};
use crate::ytdlp::{dep_deno, dep_ffmpeg, dep_ytdlp};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "dep-cache.json";
const THROTTLE_KEY: &str = "lastAutoUpdateAt";

/// Don't re-check more than once a day — a launch shouldn't hammer the GitHub API
/// or re-download on every restart.
const THROTTLE: Duration = Duration::from_secs(24 * 60 * 60);

/// ffmpeg/deno have no clean upstream version to diff against (rolling builds),
/// so they refresh on age instead. 30 days keeps them reasonably current without
/// pulling ~80MB on a frequent cadence.
const STALE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Spawn a background task that refreshes app-managed binaries if they're stale.
///
/// Non-blocking and best-effort: every failure is logged and swallowed, since the
/// seeded copy and the manual dependency tab remain as fallbacks.
pub fn auto_update_bundled_deps(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !enabled(&app) {
            return;
        }
        if throttled(&app) {
            return;
        }
        // Stamp the attempt up front so repeated launches (or a transient network
        // failure) don't retry on every single startup.
        record_check_time(&app);
        logger::info_cat("dependency", "Startup dependency auto-update check");

        // Never mutate binaries while downloads are running or queued: the startup
        // queue resume kicks in ~300ms after launch, so pending rows count as busy
        // even before active_count moves. Skip and rewind the throttle stamp so the
        // next launch retries instead of silently deferring a full day.
        if downloads_busy(&app) {
            logger::info_cat(
                "dependency",
                "Skipping dependency auto-update: downloads are active or queued",
            );
            clear_check_time(&app);
            return;
        }

        update_ytdlp_if_outdated(&app).await;
        refresh_if_aged(&app, "deno", "deno.exe").await;
        refresh_if_aged(&app, "ffmpeg", "ffmpeg.exe").await;

        invalidate_dep_cache();
    });
}

/// The `autoUpdateYtdlp` setting gates auto-update for all bundled binaries.
/// Defaults to on when unset.
fn enabled(app: &AppHandle) -> bool {
    app.store("settings.json")
        .ok()
        .and_then(|store| store.get("autoUpdateYtdlp"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

fn throttled(app: &AppHandle) -> bool {
    let last = app
        .store(STORE_FILE)
        .ok()
        .and_then(|store| store.get(THROTTLE_KEY))
        .and_then(|v| v.as_u64());
    match last {
        Some(last) => now_secs().saturating_sub(last) < THROTTLE.as_secs(),
        None => false,
    }
}

fn record_check_time(app: &AppHandle) {
    if let Ok(store) = app.store(STORE_FILE) {
        store.set(THROTTLE_KEY, serde_json::json!(now_secs()));
        let _ = store.save();
    }
}

/// Rewind the throttle stamp after a busy-skip so the next launch retries
/// immediately instead of waiting out the full 24h window.
fn clear_check_time(app: &AppHandle) {
    if let Ok(store) = app.store(STORE_FILE) {
        store.delete(THROTTLE_KEY);
        let _ = store.save();
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Absolute path of an app-managed binary, if it exists on disk.
fn app_bin(app: &AppHandle, unix_name: &str, windows_name: &str) -> Option<PathBuf> {
    let bin_dir = ensure_bin_dir(app).ok()?;
    let name = if cfg!(target_os = "windows") {
        windows_name
    } else {
        unix_name
    };
    let path = bin_dir.join(name);
    path.exists().then_some(path)
}

/// App-managed yt-dlp executable: onedir `bin/ytdlp/yt-dlp(.exe)` preferred, with
/// the legacy single-file `bin/yt-dlp(.exe)` as a fallback (mirrors resolution).
fn app_ytdlp_bin(app: &AppHandle) -> Option<PathBuf> {
    let bin_dir = ensure_bin_dir(app).ok()?;
    let exe = if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    let onedir = bin_dir.join("ytdlp").join(exe);
    if onedir.exists() {
        return Some(onedir);
    }
    let legacy = bin_dir.join(exe);
    legacy.exists().then_some(legacy)
}

/// yt-dlp has a clean version string, so diff it against the latest release and
/// re-download only when they differ — this never pulls the binary when current.
async fn update_ytdlp_if_outdated(app: &AppHandle) {
    let Some(path) = app_ytdlp_bin(app) else {
        return;
    };
    let (Some(current), Ok(latest)) = (
        get_binary_version(&path, "--version").await,
        dep_ytdlp::get_latest_version().await,
    ) else {
        // Couldn't determine either side; leave it to the manual flow.
        return;
    };
    if normalize(&current) == normalize(&latest) {
        return;
    }
    logger::info_cat(
        "dependency",
        &format!("Auto-updating yt-dlp {current} -> {latest}"),
    );
    match dep_ytdlp::install_ytdlp(app).await {
        Ok(v) => logger::info_cat("dependency", &format!("yt-dlp auto-updated to {v}")),
        Err(e) => {
            // A download that started mid-install is a deferral, not a failure.
            if is_downloads_busy_error(&e) {
                clear_check_time(app);
            }
            logger::warn_cat("dependency", &format!("yt-dlp auto-update failed: {e}"));
        }
    }
}

/// Refresh a rolling-build binary (ffmpeg/deno) when the on-disk copy is older
/// than `STALE_AGE`. Note: seeding/installing resets the file mtime, so this
/// measures age-since-install rather than age-since-build — a good-enough proxy
/// given these rarely affect YouTube downloads.
async fn refresh_if_aged(app: &AppHandle, unix_name: &str, windows_name: &str) {
    let Some(path) = app_bin(app, unix_name, windows_name) else {
        return;
    };
    if !older_than(&path, STALE_AGE) {
        return;
    }
    logger::info_cat(
        "dependency",
        &format!("Auto-updating {unix_name} (on-disk copy older than 30 days)"),
    );
    let result = match unix_name {
        "deno" => dep_deno::install_deno(app).await,
        "ffmpeg" => dep_ffmpeg::install_ffmpeg(app).await,
        _ => return,
    };
    if let Err(e) = result {
        // A download that started mid-install is a deferral, not a failure.
        if is_downloads_busy_error(&e) {
            clear_check_time(app);
        }
        logger::warn_cat(
            "dependency",
            &format!("{unix_name} auto-update failed: {e}"),
        );
    }
}

fn older_than(path: &Path, max_age: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .map(|age| age > max_age)
        .unwrap_or(false)
}

/// Compare versions ignoring a leading `v` (deno tags carry one, yt-dlp doesn't).
fn normalize(version: &str) -> &str {
    version.trim().trim_start_matches('v')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn normalize_matches_across_v_prefix_and_whitespace() {
        // deno reports the tag with a leading `v`, yt-dlp without one.
        assert_eq!(normalize("2026.03.17"), normalize(" 2026.03.17 "));
        assert_eq!(normalize("v2.8.1"), normalize("2.8.1"));
        assert_ne!(normalize("2026.03.17"), normalize("2026.02.01"));
    }

    #[test]
    fn older_than_compares_file_mtime() {
        let dir = std::env::temp_dir().join(format!("dep_autoupdate_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bin");
        fs::write(&file, b"x").unwrap();

        // Just-written file is not older than a day.
        assert!(!older_than(&file, Duration::from_secs(86_400)));
        // Every real file is older than zero seconds.
        assert!(older_than(&file, Duration::ZERO));
        // A missing file is treated as not-stale (we don't want to trigger a download).
        assert!(!older_than(&dir.join("missing"), Duration::ZERO));

        let _ = fs::remove_dir_all(&dir);
    }
}
