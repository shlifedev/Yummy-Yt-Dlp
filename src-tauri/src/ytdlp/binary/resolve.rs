use super::path::{app_bin_dir, command_with_path, is_external_mode};
use crate::modules::types::AppError;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::AppHandle;

/// Try to get version from a binary. Returns Ok(version) or Err(reason).
pub(super) async fn try_get_version(binary_path: &Path) -> Result<String, String> {
    let mut cmd = command_with_path(binary_path.to_str().unwrap_or("yt-dlp"));
    cmd.arg("--version");

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // PyInstaller binaries (yt-dlp_macos) need time to extract on first run
    let timeout_result = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await;

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

/// Resolve yt-dlp binary: app_data_dir/bin/ first (if external mode), then system PATH.
pub async fn resolve_ytdlp_path_with_app(app: &AppHandle) -> Result<String, AppError> {
    // 1. Check app-managed binary (only in external mode)
    if is_external_mode(app) {
        if let Some(bin_dir) = app_bin_dir(app) {
            let bin_name = if cfg!(target_os = "windows") {
                "yt-dlp.exe"
            } else {
                "yt-dlp"
            };
            let app_binary = bin_dir.join(bin_name);
            if app_binary.exists() {
                return Ok(app_binary.to_string_lossy().to_string());
            }
        }
    }

    // 2. Fallback to system PATH
    resolve_ytdlp_path().await
}

/// Resolve ffmpeg binary: app_data_dir/bin/ first (if external mode), then system PATH.
pub async fn resolve_ffmpeg_path_with_app(app: &AppHandle) -> Option<String> {
    // 1. Check app-managed binary (only in external mode)
    if is_external_mode(app) {
        if let Some(bin_dir) = app_bin_dir(app) {
            let bin_name = if cfg!(target_os = "windows") {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            };
            let app_binary = bin_dir.join(bin_name);
            if app_binary.exists() {
                // Return the directory containing ffmpeg
                return Some(bin_dir.to_string_lossy().to_string());
            }
        }
    }

    // 2. Fallback to system PATH
    resolve_ffmpeg_path().await
}

/// Resolve deno binary: app_data_dir/bin/ (if external mode) -> ~/.deno/bin/ -> system PATH.
pub async fn resolve_deno_path(app: &AppHandle) -> Option<PathBuf> {
    // 1. Check app-managed binary (only in external mode)
    if is_external_mode(app) {
        if let Some(bin_dir) = app_bin_dir(app) {
            let bin_name = if cfg!(target_os = "windows") {
                "deno.exe"
            } else {
                "deno"
            };
            let app_binary = bin_dir.join(bin_name);
            if app_binary.exists() {
                return Some(app_binary);
            }
        }
    }

    // 2. Check ~/.deno/bin/
    let deno_home = if cfg!(target_os = "windows") {
        std::env::var("USERPROFILE")
            .ok()
            .map(|p| PathBuf::from(p).join(".deno").join("bin").join("deno.exe"))
    } else {
        std::env::var("HOME")
            .ok()
            .map(|p| PathBuf::from(p).join(".deno").join("bin").join("deno"))
    };
    if let Some(deno_path) = deno_home {
        if deno_path.exists() {
            return Some(deno_path);
        }
    }

    // 3. Check system PATH using which/where
    let which_cmd = if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    };
    let mut cmd = command_with_path(which_cmd);
    cmd.arg("deno");

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    if let Ok(Ok(result)) = tokio::time::timeout(Duration::from_secs(5), cmd.output()).await {
        if result.status.success() {
            if let Ok(path) = String::from_utf8(result.stdout) {
                let path = path.lines().next().unwrap_or("").trim().to_string();
                if !path.is_empty() {
                    return Some(PathBuf::from(path));
                }
            }
        }
    }

    None
}

/// Check deno version from a path.
pub async fn check_deno_version(deno_path: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(deno_path);
    cmd.arg("--version");

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
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

/// Update yt-dlp using --update flag
pub async fn update_ytdlp() -> Result<String, AppError> {
    let ytdlp_path = resolve_ytdlp_path().await?;

    let mut cmd = command_with_path(&ytdlp_path);
    cmd.arg("--update");

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    let output = cmd
        .output()
        .await
        .map_err(|e| AppError::Custom(format!("Failed to update yt-dlp: {}", e)))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Custom(format!("Update failed: {}", stderr)))
    }
}
