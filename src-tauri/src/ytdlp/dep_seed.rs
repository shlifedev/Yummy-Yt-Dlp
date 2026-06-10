use crate::ytdlp::binary::{
    invalidate_dep_cache,
    path::{augmented_path, dep_mode, DepMode},
};
use crate::ytdlp::dep_download::{
    copy_dir_recursive, ensure_bin_dir, remove_quarantine, remove_quarantine_recursive,
    set_executable,
};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

/// Copy bundled yt-dlp/ffmpeg/deno from app resources into `app_data_dir/bin` on first run.
///
/// Running from a writable copy (not the signed/read-only bundle) keeps
/// `yt-dlp --update` and re-downloads working. Failures are non-fatal: the existing
/// download flow is the safety net, so we just log and move on.
///
/// Behaviour by mode:
/// - `System`: skip entirely; system PATH is the only source.
/// - `Bundled`: seed every target so the app is fully self-contained.
/// - `Hybrid`: seed only what is missing from the system PATH, so we don't ship
///   redundant copies the user already has.
pub fn seed_bundled_binaries(app: &AppHandle) {
    let mode = dep_mode(app);
    let Ok(bin_dir) = ensure_bin_dir(app) else {
        return;
    };

    // yt-dlp ships as an onedir tree, so it seeds differently from the single-file deps.
    if !(mode == DepMode::Hybrid && system_has("yt-dlp")) {
        seed_ytdlp_onedir(app, &bin_dir);
    }

    for &name in seed_single_file_targets() {
        let dest = bin_dir.join(name);
        // Never clobber a binary the user may have refreshed.
        if dest.exists() {
            continue;
        }
        // In hybrid mode a system-provided binary already satisfies this target.
        if mode == DepMode::Hybrid && system_has(strip_exe(name)) {
            continue;
        }
        let Ok(src) = app
            .path()
            .resolve(format!("binaries/{name}"), BaseDirectory::Resource)
        else {
            continue;
        };
        // Absent in dev (`tauri dev`) or unbundled builds -> fall back to download.
        if !src.exists() {
            continue;
        }
        if std::fs::copy(&src, &dest).is_ok() {
            // Bundled binaries inherit the app's quarantine; strip it so the
            // unsigned copy runs without a Gatekeeper prompt.
            let _ = set_executable(&dest);
            let _ = remove_quarantine(&dest);
            crate::modules::logger::info_cat("dependency", &format!("Seeded bundled {name}"));
        }
    }

    invalidate_dep_cache();
}

/// Seed the bundled onedir yt-dlp (`binaries/ytdlp/` -> `bin/ytdlp/`).
///
/// Prefers the onedir resource directory; falls back to a legacy single-file
/// `binaries/yt-dlp(.exe)` resource so a transitional bundle that still ships the
/// old layout keeps seeding. Skips if anything app-managed already exists on disk.
fn seed_ytdlp_onedir(app: &AppHandle, bin_dir: &std::path::Path) {
    let exe = if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    };
    let dest_dir = bin_dir.join("ytdlp");
    let legacy_dest = bin_dir.join(exe);
    // Never clobber an existing app-managed copy (onedir or legacy).
    if dest_dir.exists() || legacy_dest.exists() {
        return;
    }

    if let Ok(src_dir) = app
        .path()
        .resolve("binaries/ytdlp", BaseDirectory::Resource)
    {
        if src_dir.is_dir() {
            if copy_dir_recursive(&src_dir, &dest_dir).is_ok() {
                let _ = set_executable(&dest_dir.join(exe));
                let _ = remove_quarantine_recursive(&dest_dir);
                crate::modules::logger::info_cat("dependency", "Seeded bundled yt-dlp (onedir)");
            } else {
                // Don't leave a half-copied tree behind for resolution to trip over.
                let _ = std::fs::remove_dir_all(&dest_dir);
            }
            return;
        }
    }

    // Transitional fallback: a bundle that still ships the old single-file binary.
    if let Ok(src) = app
        .path()
        .resolve(format!("binaries/{exe}"), BaseDirectory::Resource)
    {
        if src.exists() && std::fs::copy(&src, &legacy_dest).is_ok() {
            let _ = set_executable(&legacy_dest);
            let _ = remove_quarantine(&legacy_dest);
            crate::modules::logger::info_cat("dependency", "Seeded bundled yt-dlp (legacy)");
        }
    }
}

/// Single-file binaries seeded from the bundle (ffmpeg, ffprobe, deno).
fn seed_single_file_targets() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["ffmpeg.exe", "ffprobe.exe", "deno.exe"]
    } else {
        &["ffmpeg", "ffprobe", "deno"]
    }
}

/// Drop the `.exe` suffix so a Windows target name can be looked up by `where`.
fn strip_exe(name: &str) -> &str {
    name.strip_suffix(".exe").unwrap_or(name)
}

/// Synchronous check for whether a command resolves on the augmented system PATH.
/// Used during seeding (a sync context) to decide if a bundled copy is redundant.
fn system_has(name: &str) -> bool {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut cmd = std::process::Command::new(which_cmd);
    cmd.env("PATH", augmented_path());
    cmd.arg(name);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.status().map(|s| s.success()).unwrap_or(false)
}
