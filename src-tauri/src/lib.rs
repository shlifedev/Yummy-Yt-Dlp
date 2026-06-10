mod ytdlp;

#[cfg(debug_assertions)]
use specta_typescript::{BigIntExportBehavior, Typescript};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tauri_specta::{collect_commands, collect_events};

pub mod modules {
    pub mod log_commands;
    pub mod log_db;
    pub mod logger;
    pub mod types;
}

pub type DbState = Arc<ytdlp::db::Database>;
pub type DownloadManagerState = Arc<ytdlp::download::DownloadManager>;
pub type ScanManagerState = Arc<ytdlp::metadata::ScanManager>;
pub type LogDbState = Arc<modules::log_db::LogDatabase>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            ytdlp::commands::check_dependencies,
            ytdlp::commands::update_ytdlp,
            ytdlp::commands::get_download_queue,
            ytdlp::commands::get_active_queue,
            ytdlp::commands::clear_all_queue_and_history,
            ytdlp::commands::clear_completed,
            ytdlp::commands::retry_download,
            ytdlp::commands::get_settings,
            ytdlp::commands::update_settings,
            ytdlp::commands::select_download_directory,
            ytdlp::commands::get_available_browsers,
            ytdlp::commands::get_download_history,
            ytdlp::commands::check_duplicate,
            ytdlp::commands::delete_history_item,
            ytdlp::commands::get_active_downloads,
            ytdlp::commands::get_queue_summary,
            ytdlp::commands::get_group_history_items,
            ytdlp::commands::delete_history_group,
            ytdlp::metadata::validate_url,
            ytdlp::metadata::detect_url_type,
            ytdlp::metadata::fetch_video_info,
            ytdlp::metadata::start_playlist_scan,
            ytdlp::metadata::cancel_playlist_scan,
            ytdlp::metadata::fetch_quick_metadata,
            ytdlp::download::start_download,
            ytdlp::download::add_to_queue,
            ytdlp::download::add_to_queue_batch,
            ytdlp::download::cancel_download,
            ytdlp::download::cancel_all_downloads,
            ytdlp::download::cancel_group,
            ytdlp::download::pause_download,
            ytdlp::download::resume_download,
            ytdlp::commands::set_minimize_to_tray,
            ytdlp::commands::get_recent_logs,
            ytdlp::commands::get_cached_dep_status,
            ytdlp::commands::check_full_dependencies,
            ytdlp::commands::install_dependency,
            ytdlp::commands::install_all_dependencies,
            ytdlp::commands::check_dependency_update,
            ytdlp::commands::update_dependency,
            ytdlp::commands::delete_app_managed_dep,
            ytdlp::commands::reset_all_data,
            modules::log_commands::get_logs,
            modules::log_commands::get_log_stats,
            modules::log_commands::clear_logs,
        ])
        .events(collect_events![
            ytdlp::types::GlobalDownloadEvent,
            ytdlp::types::DepInstallEvent,
            ytdlp::types::NewLogEvent,
            ytdlp::types::PlaylistScanEvent,
        ]);

    #[cfg(debug_assertions)]
    {
        builder
            .export(
                Typescript::default().bigint(BigIntExportBehavior::Number),
                "../src/lib/bindings.ts",
            )
            .expect("Failed to export typescript bindings");
    }

    let invoke_handler = builder.invoke_handler();

    #[allow(unused_mut)]
    let mut tauri_builder = tauri::Builder::default();

    // Single-instance must be the FIRST plugin so a second launch exits before its
    // setup() runs reset_stale_downloads() against the first instance's live queue.
    // The callback unhides the (possibly tray-hidden) window of the running instance.
    #[cfg(desktop)]
    {
        tauri_builder =
            tauri_builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }));
    }

    tauri_builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            builder.mount_events(app);
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            modules::logger::init(app_data_dir.clone());

            // Initialize log database (separate logs.db file). A corrupt/unopenable
            // logs.db must not brick startup: quarantine + retry, then in-memory fallback.
            let log_db = open_db_resilient(
                &app_data_dir,
                "logs.db",
                || modules::log_db::LogDatabase::new(&app_data_dir),
                modules::log_db::LogDatabase::new_in_memory,
            )?;
            let log_db = Arc::new(log_db);
            modules::logger::init_db(Arc::clone(&log_db));
            modules::logger::init_app_handle(app.handle().clone());
            app.manage(log_db.clone());

            // Cleanup old logs on startup (30 days, 50k max)
            if let Err(e) = log_db.cleanup_old_logs(30, 50000) {
                eprintln!("Failed to cleanup old logs: {}", e);
            }

            modules::logger::info_cat("app", "Application started");

            // A corrupt/unopenable ytdlp.db must not brick startup either: quarantine the
            // bad file (never delete it) + retry, then in-memory fallback as last resort.
            let db = open_db_resilient(
                &app_data_dir,
                "ytdlp.db",
                || ytdlp::db::Database::new(&app_data_dir),
                ytdlp::db::Database::new_in_memory,
            )?;
            // Reset stale downloads left in 'downloading' state from previous session
            if let Ok(count) = db.reset_stale_downloads() {
                if count > 0 {
                    modules::logger::info_cat(
                        "app",
                        &format!("Reset {} stale downloads from previous session", count),
                    );
                }
            }
            // Prune terminal queue rows (completed/failed/cancelled) older than 30 days so
            // the downloads table — polled in full by the queue page — stays bounded.
            // Completed items are already mirrored into history, so nothing is lost.
            match db.prune_old_terminal_downloads(30) {
                Ok(count) if count > 0 => {
                    modules::logger::info_cat(
                        "app",
                        &format!("Pruned {} finished queue entries older than 30 days", count),
                    );
                }
                Err(e) => {
                    modules::logger::warn_cat(
                        "app",
                        &format!("Failed to prune old queue entries: {}", e),
                    );
                }
                _ => {}
            }
            app.manage(Arc::new(db));

            // Initialize DownloadManager with max_concurrent from settings
            let settings =
                ytdlp::settings::get_settings_from_path(&app_data_dir).unwrap_or_default();
            let download_manager = Arc::new(ytdlp::download::DownloadManager::new(
                settings.max_concurrent,
            ));
            app.manage(download_manager);

            app.manage(Arc::new(ytdlp::metadata::ScanManager::default()));

            // Setup system tray. Treat tray setup as best-effort: if the OS doesn't provide a
            // window icon (or tray creation otherwise fails), log and continue rather than
            // aborting launch — a missing tray must not brick the whole app.
            if let Err(e) = ytdlp::tray::setup_tray(&app.handle().clone()) {
                modules::logger::warn_cat(
                    "app",
                    &format!("Failed to setup system tray (continuing without it): {}", e),
                );
            }

            // Seed bundled yt-dlp/ffmpeg into app_data_dir/bin before warmup/dep checks.
            // Runs from a writable copy so `yt-dlp --update` keeps working; deno stays dynamic.
            ytdlp::dep_seed::seed_bundled_binaries(app.handle());

            // Keep the seeded copies fresh: a background, throttled check that
            // re-downloads an outdated yt-dlp (and stale ffmpeg/deno) so a frozen
            // bundle doesn't silently break downloads. Gated by `autoUpdateYtdlp`.
            ytdlp::dep_autoupdate::auto_update_bundled_deps(app.handle());

            // Process any pending downloads left from a previous session.
            // These are items that were 'pending' (not 'downloading') when the app closed,
            // so reset_stale_downloads() does not touch them.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Small delay to let the app fully initialize before processing
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                ytdlp::download::process_next_pending_public(handle);
            });

            // Warmup yt-dlp in background to prime OS file cache (PyInstaller cold start mitigation)
            ytdlp::binary::warmup_ytdlp(app.handle().clone());

            Ok(())
        })
        .invoke_handler(invoke_handler)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let setting = ytdlp::tray::get_minimize_to_tray_setting(app);
                match setting {
                    Some(true) => {
                        // Minimize to tray
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    Some(false) => {
                        // Closing is allowed, but warn first if downloads are still running so
                        // the user doesn't silently lose in-progress work. The frontend shows a
                        // confirm dialog and, on confirm, exits via the process plugin.
                        let manager = app.state::<DownloadManagerState>();
                        let active = manager.active_count();
                        if active > 0 {
                            api.prevent_close();
                            let _ = app.emit("close-blocked", active);
                        }
                        // active == 0: let the window close normally
                        // (cancel_all runs in RunEvent::Exit).
                    }
                    None => {
                        // Not decided yet: prevent close and ask frontend
                        api.prevent_close();
                        let _ = app.emit("close-requested", ());
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                let manager = app_handle.state::<DownloadManagerState>();
                let scan_manager = app_handle.state::<ScanManagerState>();
                // Stop new work from being claimed while draining, then signal every
                // in-flight download/scan and give their executor tasks a bounded
                // window to run kill_process_tree (they run on tokio workers, which
                // keep being polled while the main thread blocks here). Without the
                // wait, the process exits before any kill runs and yt-dlp/ffmpeg
                // survive as orphans.
                manager.shutdown();
                manager.cancel_all();
                scan_manager.cancel_current();
                tauri::async_runtime::block_on(async {
                    let _ = tokio::join!(
                        manager.wait_until_idle(std::time::Duration::from_secs(3)),
                        scan_manager.wait_until_idle(std::time::Duration::from_secs(3)),
                    );
                });
            }
        });
}

/// True for SQLite corruption-class open failures (SQLITE_NOTADB / SQLITE_CORRUPT), where
/// the on-disk file is unusable and quarantining it is the only way forward. Transient
/// failures (disk full, locked, permissions) must NOT match — the data is intact there.
fn is_db_corruption_error(message: &str) -> bool {
    message.contains("file is not a database")
        || message.contains("database disk image is malformed")
}

/// Move `file_name` and its `-wal`/`-shm` siblings aside as `*.corrupt-<timestamp>`.
/// Returns true only if every existing file was moved — a stale -wal left next to a fresh
/// db file would itself make the retry fail. Never deletes user data.
fn quarantine_db_files(app_data_dir: &std::path::Path, file_name: &str) -> bool {
    let ts = chrono::Utc::now().timestamp();
    let mut all_moved = true;
    for suffix in ["", "-wal", "-shm"] {
        let src = app_data_dir.join(format!("{}{}", file_name, suffix));
        if src.exists() {
            let dst = app_data_dir.join(format!("{}{}.corrupt-{}", file_name, suffix, ts));
            if let Err(e) = std::fs::rename(&src, &dst) {
                eprintln!(
                    "[Startup] Failed to quarantine {}: {}",
                    src.to_string_lossy(),
                    e
                );
                all_moved = false;
            }
        }
    }
    all_moved
}

/// Open a database with corruption recovery so one bad file can't permanently brick
/// startup: on a corruption-class error, quarantine the file (plus WAL/SHM siblings) and
/// retry once; on transient errors or a failed retry, fall back to an in-memory database
/// for this session. The in-memory fallback is required — lib.rs manages the handles as
/// Tauri state, and skipping app.manage() would panic every command at invoke time.
/// Logged via modules::logger: logger::init runs before either DB init, so the ytdlp.db
/// quarantine reaches logs.db + the live log view, while a logs.db quarantine can only
/// reach log.txt.
fn open_db_resilient<T>(
    app_data_dir: &std::path::Path,
    file_name: &str,
    open: impl Fn() -> Result<T, modules::types::AppError>,
    open_in_memory: impl FnOnce() -> Result<T, modules::types::AppError>,
) -> Result<T, modules::types::AppError> {
    let err = match open() {
        Ok(db) => return Ok(db),
        Err(e) => e,
    };

    let msg = err.to_string();
    if is_db_corruption_error(&msg) {
        modules::logger::error_cat(
            "app",
            &format!(
                "{} is corrupted ({}); moving it aside and recreating",
                file_name, msg
            ),
        );
        if quarantine_db_files(app_data_dir, file_name) {
            match open() {
                Ok(db) => {
                    modules::logger::warn_cat(
                        "app",
                        &format!(
                            "{} recreated after corruption; previous data kept as {}.corrupt-*",
                            file_name, file_name
                        ),
                    );
                    return Ok(db);
                }
                Err(e) => {
                    modules::logger::error_cat(
                        "app",
                        &format!("Failed to recreate {} after quarantine: {}", file_name, e),
                    );
                }
            }
        }
    } else {
        modules::logger::error_cat("app", &format!("Failed to open {}: {}", file_name, msg));
    }

    modules::logger::error_cat(
        "app",
        &format!(
            "Falling back to an in-memory {} for this session; data will not persist",
            file_name
        ),
    );
    open_in_memory()
}
