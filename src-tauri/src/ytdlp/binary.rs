use super::types::{DepInfo, DepSource, DependencyStatus, FullDependencyStatus};
use crate::modules::types::AppError;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
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

/// Get the dep_mode from settings. Defaults to "external".
fn get_dep_mode(app: &AppHandle) -> String {
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
fn is_external_mode(app: &AppHandle) -> bool {
    get_dep_mode(app) == "external"
}

/// Get the app-managed bin directory path.
fn app_bin_dir(app: &AppHandle) -> Option<PathBuf> {
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
                // check_dep_ytdlp과 일관성 유지: 바이너리 존재만 확인
                // PyInstaller 첫 실행 추출 타임아웃, Gatekeeper 등으로 버전 체크가 실패할 수 있으나
                // 실제 다운로드 명령은 정상 동작할 수 있음
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

async fn check_dep_ytdlp(app: &AppHandle) -> DepInfo {
    // Check app-managed first (only in external mode)
    if is_external_mode(app) {
        if let Some(bin_dir) = app_bin_dir(app) {
            let bin_name = if cfg!(target_os = "windows") {
                "yt-dlp.exe"
            } else {
                "yt-dlp"
            };
            let app_binary = bin_dir.join(bin_name);
            if app_binary.exists() {
                // Binary file exists in app bin dir — report as installed.
                // Version check may fail on first run (PyInstaller extraction, Gatekeeper, etc.)
                let version = try_get_version(&app_binary).await.ok();
                return DepInfo {
                    installed: true,
                    version,
                    source: DepSource::AppManaged,
                    path: Some(app_binary.to_string_lossy().to_string()),
                };
            }
        }
    }

    // Quick existence check via which/where before spawning yt-dlp --version
    if !quick_binary_exists("yt-dlp").await {
        return DepInfo {
            installed: false,
            version: None,
            source: DepSource::NotFound,
            path: None,
        };
    }

    // Check system PATH
    let (version, _debug) = check_ytdlp().await;
    if let Some(ver) = version {
        DepInfo {
            installed: true,
            version: Some(ver),
            source: DepSource::SystemPath,
            path: None,
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

async fn check_dep_ffmpeg(app: &AppHandle) -> DepInfo {
    // Check app-managed first (only in external mode)
    if is_external_mode(app) {
        if let Some(bin_dir) = app_bin_dir(app) {
            let bin_name = if cfg!(target_os = "windows") {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            };
            let app_binary = bin_dir.join(bin_name);
            if app_binary.exists() {
                let mut version: Option<String> = None;
                let mut cmd = tokio::process::Command::new(&app_binary);
                cmd.arg("-version");
                #[cfg(target_os = "windows")]
                {
                    cmd.creation_flags(0x08000000);
                }
                if let Ok(Ok(output)) =
                    tokio::time::timeout(Duration::from_secs(5), cmd.output()).await
                {
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
                // Binary file exists in app bin dir — report as installed
                return DepInfo {
                    installed: true,
                    version,
                    source: DepSource::AppManaged,
                    path: Some(app_binary.to_string_lossy().to_string()),
                };
            }
        }
    }

    // Check system PATH
    if let Some(version) = check_ffmpeg().await {
        DepInfo {
            installed: true,
            version: Some(version),
            source: DepSource::SystemPath,
            path: None,
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

async fn check_dep_deno(app: &AppHandle) -> DepInfo {
    if let Some(deno_path) = resolve_deno_path(app).await {
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

/// Warmup yt-dlp by running `--version` in the background.
/// PyInstaller `--onefile` binaries need to extract the Python runtime on each run;
/// triggering this early primes the OS file cache so subsequent invocations are faster.
pub fn warmup_ytdlp(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let path = match resolve_ytdlp_path_with_app(&app).await {
            Ok(p) => p,
            Err(_) => return,
        };
        let mut cmd = command_with_path_app(&path, &app);
        cmd.arg("--version");

        #[cfg(target_os = "windows")]
        {
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // Result is intentionally ignored; this is purely for OS file cache priming.
        let _ = tokio::time::timeout(Duration::from_secs(30), cmd.output()).await;
    });
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
