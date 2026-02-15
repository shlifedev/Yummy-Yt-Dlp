use super::types::DependencyStatus;
use crate::modules::types::AppError;
use std::path::Path;
use std::time::Duration;

/// Platform-specific PATH separator.
const PATH_SEP: &str = if cfg!(target_os = "windows") {
    ";"
} else {
    ":"
};

/// Build an augmented PATH that includes common package manager locations.
/// Bundled desktop apps often don't inherit the user's full shell PATH.
fn augmented_path() -> String {
    let current = std::env::var("PATH").unwrap_or_default();

    let mut extra: Vec<String> = Vec::new();

    if cfg!(target_os = "windows") {
        // Windows: resolve user-specific paths at runtime
        if let Ok(profile) = std::env::var("USERPROFILE") {
            // winget
            extra.push(format!(r"{}\AppData\Local\Microsoft\WinGet\Links", profile));
            // scoop
            extra.push(format!(r"{}\scoop\shims", profile));
            // pip (common Python versions)
            for ver in &["313", "312", "311", "310"] {
                extra.push(format!(
                    r"{}\AppData\Local\Programs\Python\Python{}\Scripts",
                    profile, ver
                ));
            }
            extra.push(format!(
                r"{}\AppData\Local\Programs\Python\Python3\Scripts",
                profile
            ));
            // pipx
            extra.push(format!(r"{}\.local\bin", profile));
        }
        // chocolatey
        extra.push(r"C:\ProgramData\chocolatey\bin".to_string());
    } else {
        // macOS / Linux
        extra.push("/opt/homebrew/bin".to_string()); // brew (Apple Silicon)
        extra.push("/usr/local/bin".to_string()); // brew (Intel Mac) / common Linux
        extra.push("/usr/bin".to_string());
        extra.push("/bin".to_string());
        // pip install --user
        if let Ok(home) = std::env::var("HOME") {
            extra.push(format!("{}/.local/bin", home));
        }
    }

    // Prepend extra dirs, then append original PATH
    if !current.is_empty() {
        extra.push(current);
    }
    extra.join(PATH_SEP)
}

/// Create a Command with augmented PATH and Python UTF-8 environment variables.
///
/// - `PYTHONUTF8=1`: Forces all text I/O to UTF-8 (PEP 540), fixes cp949 file I/O errors on Korean Windows
/// - `PYTHONIOENCODING=utf-8`: Forces stdin/stdout/stderr to UTF-8
/// - `PYTHONUNBUFFERED=1`: Disables stdout buffering for real-time progress output
///
/// These are harmless no-ops for non-Python programs (ffmpeg, where/which).
pub fn command_with_path(program: &str) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.env("PATH", augmented_path());
    cmd.env("PYTHONUTF8", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.env("PYTHONUNBUFFERED", "1");
    // pip-installed yt-dlp (system Python): LANG forces UTF-8 locale on Windows
    #[cfg(target_os = "windows")]
    {
        cmd.env("LANG", "en_US.UTF-8");
    }
    cmd
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
    let path_env = augmented_path();
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

/// Try to get version from a binary. Returns Ok(version) or Err(reason).
async fn try_get_version(binary_path: &Path) -> Result<String, String> {
    let mut cmd = command_with_path(binary_path.to_str().unwrap_or("yt-dlp"));
    cmd.arg("--version");

    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // PyInstaller binaries (yt-dlp_macos) need time to extract on first run
    let timeout_result = tokio::time::timeout(Duration::from_secs(30), cmd.output()).await;

    let cmd_result = match timeout_result {
        Ok(result) => result,
        Err(_) => {
            return Err(format!("timeout (30s) executing {}", binary_path.display()));
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

        if let Ok(Ok(result)) =
            tokio::time::timeout(Duration::from_secs(5), which.output()).await
        {
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
pub async fn check_dependencies() -> DependencyStatus {
    let (ytdlp_version, debug_lines) = check_ytdlp().await;
    let ffmpeg_version = check_ffmpeg().await;

    let debug_text = if debug_lines.is_empty() {
        None
    } else {
        Some(debug_lines.join("\n"))
    };

    DependencyStatus {
        ytdlp_installed: ytdlp_version.is_some(),
        ytdlp_version,
        ffmpeg_installed: ffmpeg_version.is_some(),
        ffmpeg_version,
        ytdlp_debug: debug_text,
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
