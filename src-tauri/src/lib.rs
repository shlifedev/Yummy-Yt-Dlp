mod command;
mod ytdlp;

#[cfg(debug_assertions)]
use specta_typescript::{BigIntExportBehavior, Typescript};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_specta::{collect_commands, collect_events};

pub mod modules {
    pub mod logger;
    pub mod types;
}

pub struct AppState {}

pub type DbState = Arc<ytdlp::db::Database>;
pub type DownloadManagerState = Arc<ytdlp::download::DownloadManager>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            command::increment_counter,
            ytdlp::commands::check_dependencies,
            ytdlp::commands::update_ytdlp,
            ytdlp::commands::get_download_queue,
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
            ytdlp::metadata::validate_url,
            ytdlp::metadata::fetch_video_info,
            ytdlp::metadata::fetch_playlist_info,
            ytdlp::download::start_download,
            ytdlp::download::add_to_queue,
            ytdlp::download::cancel_download,
            ytdlp::download::cancel_all_downloads,
            ytdlp::download::pause_download,
            ytdlp::download::resume_download,
            ytdlp::commands::set_minimize_to_tray,
            ytdlp::commands::get_recent_logs,
            ytdlp::commands::check_full_dependencies,
            ytdlp::commands::install_dependency,
            ytdlp::commands::install_all_dependencies,
            ytdlp::commands::check_dependency_update,
            ytdlp::commands::update_dependency,
            ytdlp::commands::delete_app_managed_dep,
        ])
        .events(collect_events![
            ytdlp::types::GlobalDownloadEvent,
            ytdlp::types::DepInstallEvent,
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

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .manage(Mutex::new(AppState {}))
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

            let db =
                ytdlp::db::Database::new(&app_data_dir).expect("Failed to initialize database");
            // Reset stale downloads left in 'downloading' state from previous session
            if let Ok(count) = db.reset_stale_downloads() {
                if count > 0 {
                    modules::logger::info(&format!(
                        "Reset {} stale downloads from previous session",
                        count
                    ));
                }
            }
            app.manage(Arc::new(db));

            // Initialize DownloadManager with max_concurrent from settings
            let settings =
                ytdlp::settings::get_settings_from_path(&app_data_dir).unwrap_or_default();
            let download_manager = Arc::new(ytdlp::download::DownloadManager::new(
                settings.max_concurrent,
            ));
            app.manage(download_manager);

            // Setup system tray
            ytdlp::tray::setup_tray(&app.handle().clone()).expect("Failed to setup system tray");

            // Process any pending downloads left from a previous session.
            // These are items that were 'pending' (not 'downloading') when the app closed,
            // so reset_stale_downloads() does not touch them.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Small delay to let the app fully initialize before processing
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                ytdlp::download::process_next_pending_public(handle);
            });

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
                        // Let window close normally (cancel_all runs in RunEvent::Exit)
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
                manager.cancel_all();
            }
        });
}
