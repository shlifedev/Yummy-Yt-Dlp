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
pub(crate) fn augmented_path() -> String {
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

/// Dependency resolution strategy.
///
/// - `Hybrid`: prefer system PATH, fall back to bundled binaries in app bin dir.
/// - `Bundled`: prefer bundled binaries in app bin dir, fall back to system PATH.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum DepMode {
    Hybrid,
    Bundled,
}

/// Resolve the dependency mode from settings.
///
/// Defaults to `Hybrid`. Legacy `"external"` maps to `Bundled`; the removed
/// `"system"` value falls through to `Hybrid`.
pub(crate) fn dep_mode(app: &AppHandle) -> DepMode {
    let raw = app.store("settings.json").ok().and_then(|store| {
        store
            .get("depMode")
            .and_then(|v| v.as_str().map(String::from))
    });
    match raw.as_deref() {
        Some("bundled") | Some("external") => DepMode::Bundled,
        _ => DepMode::Hybrid,
    }
}

/// Get the app-managed bin directory path.
pub(super) fn app_bin_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("bin"))
}

/// Build a PATH string that prepends the app bin dir to the augmented PATH.
/// Used by `Bundled` mode so the app-managed binaries win over system ones.
fn augmented_path_with_app(app: &AppHandle) -> String {
    let base = augmented_path();
    if let Some(bin_dir) = app_bin_dir(app) {
        let bin_str = bin_dir.to_string_lossy().to_string();
        format!("{}{}{}", bin_str, PATH_SEP, base)
    } else {
        base
    }
}

/// Build a PATH string that appends the app bin dir after the augmented PATH.
/// Used by `Hybrid` mode so system binaries win and the bundled copies only
/// serve as a fallback when nothing is found on the system.
fn augmented_path_app_suffix(app: &AppHandle) -> String {
    let base = augmented_path();
    if let Some(bin_dir) = app_bin_dir(app) {
        let bin_str = bin_dir.to_string_lossy().to_string();
        format!("{}{}{}", base, PATH_SEP, bin_str)
    } else {
        base
    }
}

/// Create a Command with a PATH built according to the active dependency mode.
pub fn command_with_path_app(program: &str, app: &AppHandle) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    let path = match dep_mode(app) {
        DepMode::Bundled => augmented_path_with_app(app),
        DepMode::Hybrid => augmented_path_app_suffix(app),
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
