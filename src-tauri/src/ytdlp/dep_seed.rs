use crate::ytdlp::binary::{
    invalidate_dep_cache,
    path::{augmented_path, dep_mode, DepMode},
};
use crate::ytdlp::dep_download::{ensure_bin_dir, remove_quarantine, set_executable};
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

    for &name in seed_targets() {
        let dest = bin_dir.join(name);
        // Never clobber a binary the user may have refreshed via --update.
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

/// Binaries seeded from the bundle (yt-dlp, ffmpeg, ffprobe, deno).
fn seed_targets() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["yt-dlp.exe", "ffmpeg.exe", "ffprobe.exe", "deno.exe"]
    } else {
        &["yt-dlp", "ffmpeg", "ffprobe", "deno"]
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
