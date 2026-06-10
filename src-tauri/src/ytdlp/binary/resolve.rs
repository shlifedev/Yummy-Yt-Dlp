use super::path::{app_bin_dir, command_with_path, source_order, DepSourcePref};
use crate::modules::types::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

/// Try to get version from a binary. Returns Ok(version) or Err(reason).
pub(super) async fn try_get_version(binary_path: &Path) -> Result<String, String> {
    let mut cmd = command_with_path(binary_path.to_str().unwrap_or("yt-dlp"));
    cmd.arg("--version");
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // The first run of a freshly extracted onedir build still does a one-time
    // PyInstaller bootstrap and (on macOS) a Gatekeeper scan — measured ~9s — so
    // give it generous headroom; warmed-up runs return in ~0.2s.
    let timeout_result = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await;

    let cmd_result = match timeout_result {
        Ok(result) => result,
        Err(_) => {
            return Err(format!("timeout (10s) executing {}", binary_path.display()));
        }
    };

    let output = match cmd_result {
        Ok(output) => output,
        Err(e) => {
            return Err(format!("exec error: {} ({})", e, e.kind()));
        }
    };

    if output.status.success() {
        String::from_utf8(output.stdout)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("invalid utf8 in stdout: {}", e))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "exit code={}, stderr={}",
            output.status,
            stderr.trim()
        ))
    }
}

/// Resolve the yt-dlp binary from system PATH (augmented).
pub async fn resolve_ytdlp_path() -> Result<String, AppError> {
    if try_get_version(Path::new("yt-dlp")).await.is_ok() {
        return Ok("yt-dlp".to_string());
    }
    Err(AppError::BinaryNotFound(
        "yt-dlp not found. Please install via your package manager (e.g. brew install yt-dlp)."
            .to_string(),
    ))
}

/// Absolute path of the first `name` match on the augmented system PATH.
///
/// Uses the base augmented PATH (no app bin dir), so this only ever resolves a
/// genuine system-installed copy, never an app-managed one.
pub(super) async fn which_first(name: &str) -> Option<String> {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut cmd = command_with_path(which_cmd);
    cmd.arg(name);
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    let output = tokio::time::timeout(Duration::from_secs(5), cmd.output())
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

/// Check if yt-dlp is installed, return (version, debug_info).
pub async fn check_ytdlp() -> (Option<String>, Vec<String>) {
    let mut debug_lines: Vec<String> = Vec::new();
    let path_env = std::env::var("PATH").unwrap_or_default();
    debug_lines.push(format!("PATH: {}", path_env));

    debug_lines.push("checking: yt-dlp --version".to_string());
    match try_get_version(Path::new("yt-dlp")).await {
        Ok(version) => {
            debug_lines.push(format!("  OK: {}", version));
            (Some(version), debug_lines)
        }
        Err(reason) => {
            debug_lines.push(format!("  FAIL: {}", reason));

            // Platform-specific diagnostic hints
            let hint_paths: Vec<String> = if cfg!(target_os = "windows") {
                let profile = std::env::var("USERPROFILE").unwrap_or_default();
                vec![
                    format!(
                        r"{}\AppData\Local\Microsoft\WinGet\Links\yt-dlp.exe",
                        profile
                    ),
                    format!(r"{}\scoop\shims\yt-dlp.exe", profile),
                    r"C:\ProgramData\chocolatey\bin\yt-dlp.exe".to_string(),
                ]
            } else {
                vec![
                    "/opt/homebrew/bin/yt-dlp".to_string(),
                    "/usr/local/bin/yt-dlp".to_string(),
                ]
            };

            for p in &hint_paths {
                let exists = std::path::Path::new(p).exists();
                debug_lines.push(format!("  {} exists={}", p, exists));
            }

            (None, debug_lines)
        }
    }
}

/// Check if ffmpeg is installed on system PATH (augmented), return version if so.
pub async fn check_ffmpeg() -> Option<String> {
    let mut cmd = command_with_path("ffmpeg");
    cmd.arg("-version");
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = tokio::time::timeout(Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;

    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .and_then(|s| s.lines().next().map(|line| line.to_string()))
    } else {
        None
    }
}

/// Resolve the ffmpeg binary path. Returns Some(path) if ffmpeg is found on augmented PATH.
/// Used to pass --ffmpeg-location to yt-dlp for reliability on Windows.
pub async fn resolve_ffmpeg_path() -> Option<String> {
    let mut cmd = command_with_path("ffmpeg");
    cmd.arg("-version");
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = tokio::time::timeout(Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;

    if output.status.success() {
        // Try to find the actual binary path using `where` (Windows) or `which` (Unix)
        let which_cmd = if cfg!(target_os = "windows") {
            "where"
        } else {
            "which"
        };
        let mut which = command_with_path(which_cmd);
        which.arg("ffmpeg");
        which.kill_on_drop(true);

        #[cfg(target_os = "windows")]
        {
            which.creation_flags(0x08000000);
        }

        if let Ok(Ok(result)) = tokio::time::timeout(Duration::from_secs(5), which.output()).await {
            if result.status.success() {
                if let Ok(path) = String::from_utf8(result.stdout) {
                    let path = path.lines().next().unwrap_or("").trim().to_string();
                    if !path.is_empty() {
                        // Return the directory containing ffmpeg, not the binary itself
                        if let Some(parent) = std::path::Path::new(&path).parent() {
                            return Some(parent.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
        // Fallback: ffmpeg is on PATH but we can't resolve the exact location
        None
    } else {
        None
    }
}

/// Get full dependency status
pub async fn check_dependencies() -> super::super::types::DependencyStatus {
    let (ytdlp_version, debug_lines) = check_ytdlp().await;
    let ffmpeg_version = check_ffmpeg().await;

    let debug_text = if debug_lines.is_empty() {
        None
    } else {
        Some(debug_lines.join("\n"))
    };

    super::super::types::DependencyStatus {
        ytdlp_installed: ytdlp_version.is_some(),
        ytdlp_version,
        ffmpeg_installed: ffmpeg_version.is_some(),
        ffmpeg_version,
        ytdlp_debug: debug_text,
    }
}

/// Path to an app-managed binary (with platform `.exe` suffix), if it exists on disk.
pub(super) fn app_managed_binary(
    app: &AppHandle,
    unix_name: &str,
    windows_name: &str,
) -> Option<PathBuf> {
    let bin_dir = app_bin_dir(app)?;
    let name = if cfg!(target_os = "windows") {
        windows_name
    } else {
        unix_name
    };
    let path = bin_dir.join(name);
    path.exists().then_some(path)
}

/// Platform executable name for app-managed yt-dlp.
fn ytdlp_exe_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

/// Resolve the app-managed yt-dlp executable on disk, if any.
///
/// Prefers the onedir layout `bin/ytdlp/yt-dlp(.exe)`; falls back to the legacy
/// `--onefile` binary `bin/yt-dlp(.exe)` left by older installs so upgrades keep
/// working until the next reinstall replaces it with the onedir tree.
pub(super) fn app_managed_ytdlp_binary(app: &AppHandle) -> Option<PathBuf> {
    let bin_dir = app_bin_dir(app)?;
    let exe = ytdlp_exe_name();
    let onedir = bin_dir.join("ytdlp").join(exe);
    if onedir.exists() {
        return Some(onedir);
    }
    let legacy = bin_dir.join(exe);
    legacy.exists().then_some(legacy)
}

/// Resolve yt-dlp per the effective source order for it.
///
/// Always returns an absolute path so the launched copy is decided here, not by
/// PATH ordering (which `command_with_path_app` tunes for deno discovery).
pub async fn resolve_ytdlp_path_with_app(app: &AppHandle) -> Result<String, AppError> {
    let app_path = app_managed_ytdlp_binary(app).map(|p| p.to_string_lossy().to_string());
    let system_path = which_first("yt-dlp").await;

    for src in source_order(app, "yt-dlp") {
        match src {
            DepSourcePref::AppManaged => {
                if let Some(path) = &app_path {
                    return Ok(path.clone());
                }
            }
            DepSourcePref::SystemPath => {
                if let Some(path) = &system_path {
                    return Ok(path.clone());
                }
            }
        }
    }

    Err(AppError::BinaryNotFound(
        "yt-dlp not found. Please install via your package manager (e.g. brew install yt-dlp)."
            .to_string(),
    ))
}

/// Resolve the ffmpeg directory (for `--ffmpeg-location`) per the effective source order.
pub async fn resolve_ffmpeg_path_with_app(app: &AppHandle) -> Option<String> {
    // Skip an app-managed copy the last dependency check found broken — passing a
    // dead ffmpeg via --ffmpeg-location would fail every merge even when a working
    // system copy exists. The marker resets on invalidate_dep_cache() (run after
    // every install/update/delete), so a fresh install is picked up immediately.
    let app_dir = if super::dep_check::app_ffmpeg_probe_broken() {
        None
    } else {
        app_managed_binary(app, "ffmpeg", "ffmpeg.exe")
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_string()))
    };
    let system_dir = resolve_ffmpeg_path().await;

    for src in source_order(app, "ffmpeg") {
        match src {
            DepSourcePref::AppManaged => {
                if let Some(dir) = &app_dir {
                    return Some(dir.clone());
                }
            }
            DepSourcePref::SystemPath => {
                if let Some(dir) = &system_dir {
                    return Some(dir.clone());
                }
            }
        }
    }
    None
}

/// deno installed at the default `~/.deno/bin` location, if present.
pub(super) fn deno_home_path() -> Option<PathBuf> {
    let (var, exe) = if cfg!(target_os = "windows") {
        ("USERPROFILE", "deno.exe")
    } else {
        ("HOME", "deno")
    };
    let path = PathBuf::from(std::env::var(var).ok()?)
        .join(".deno")
        .join("bin")
        .join(exe);
    path.exists().then_some(path)
}

/// deno discovered on the system PATH via which/where.
pub(super) async fn deno_on_system_path() -> Option<PathBuf> {
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut cmd = command_with_path(which_cmd);
    cmd.arg("deno");
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    let output = tokio::time::timeout(Duration::from_secs(5), cmd.output())
        .await
        .ok()?
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout)
        .ok()?
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Check deno version from a path.
pub async fn check_deno_version(deno_path: &Path) -> Option<String> {
    let mut cmd = super::path::command_with_path(deno_path.to_str().unwrap_or("deno"));
    cmd.arg("--version");
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000);
    }

    let output = tokio::time::timeout(Duration::from_secs(10), cmd.output())
        .await
        .ok()?
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // deno --version outputs: "deno 1.x.x (....)" on first line
        stdout.lines().next().map(|l| l.trim().to_string())
    } else {
        None
    }
}

/// Update yt-dlp.
///
/// onedir (and the legacy onefile) builds don't support `yt-dlp --update` — the
/// self-updater can't rewrite the running executable inside the extracted tree.
/// So when yt-dlp is app-managed we re-run the installer, which re-downloads the
/// latest onedir zip and swaps it in. Only a genuine system-PATH yt-dlp (pip/brew
/// build) keeps the `--update` path.
pub async fn update_ytdlp(app: &AppHandle) -> Result<String, AppError> {
    if app_managed_ytdlp_binary(app).is_some() {
        return crate::ytdlp::dep_ytdlp::install_ytdlp(app).await;
    }

    let ytdlp_path = resolve_ytdlp_path().await?;

    let mut cmd = command_with_path(&ytdlp_path);
    cmd.arg("--update");
    cmd.kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // `--update` downloads a full binary, so allow generous time — but never hang
    // forever: an unbounded stall here would leave the update UI spinning until
    // the app is restarted.
    let output = tokio::time::timeout(Duration::from_secs(300), cmd.output())
        .await
        .map_err(|_| {
            AppError::DependencyInstallError(
                "yt-dlp --update timed out after 5 minutes".to_string(),
            )
        })?
        .map_err(|e| AppError::Custom(format!("Failed to update yt-dlp: {}", e)))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Custom(format!("Update failed: {}", stderr)))
    }
}
