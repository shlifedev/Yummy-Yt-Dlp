use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

/// Platform-specific PATH separator.
pub(super) const PATH_SEP: &str = if cfg!(target_os = "windows") {
    ";"
} else {
    ":"
};

/// Build an augmented PATH that includes common package manager locations.
/// Bundled desktop apps often don't inherit the user's full shell PATH.
pub(super) fn augmented_path() -> String {
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
            // deno default install location
            extra.push(format!(r"{}\.deno\bin", profile));
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
            // deno default install location
            extra.push(format!("{}/.deno/bin", home));
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

/// Get the dep_mode from settings. Defaults to "external".
pub(super) fn get_dep_mode(app: &AppHandle) -> String {
    app.store("settings.json")
        .ok()
        .and_then(|store| {
            store
                .get("depMode")
                .and_then(|v| v.as_str().map(String::from))
        })
        .unwrap_or_else(|| "external".to_string())
}

/// Check if app-managed binaries should be used (dep_mode == "external").
pub(super) fn is_external_mode(app: &AppHandle) -> bool {
    get_dep_mode(app) == "external"
}

/// Get the app-managed bin directory path.
pub(super) fn app_bin_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("bin"))
}

/// Build a PATH string that prepends app bin dir to the augmented PATH.
fn augmented_path_with_app(app: &AppHandle) -> String {
    let base = augmented_path();
    if let Some(bin_dir) = app_bin_dir(app) {
        let bin_str = bin_dir.to_string_lossy().to_string();
        format!("{}{}{}", bin_str, PATH_SEP, base)
    } else {
        base
    }
}

/// Create a Command with app-managed bin dir prepended to PATH (if external mode).
pub fn command_with_path_app(program: &str, app: &AppHandle) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    let path = if is_external_mode(app) {
        augmented_path_with_app(app)
    } else {
        augmented_path()
    };
    cmd.env("PATH", path);
    cmd.env("PYTHONUTF8", "1");
    cmd.env("PYTHONIOENCODING", "utf-8");
    cmd.env("PYTHONUNBUFFERED", "1");
    #[cfg(target_os = "windows")]
    {
        cmd.env("LANG", "en_US.UTF-8");
    }
    cmd
}
