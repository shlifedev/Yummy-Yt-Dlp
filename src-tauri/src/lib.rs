mod command;
mod ytdlp;

#[cfg(debug_assertions)]
use specta_typescript::{BigIntExportBehavior, Typescript};
use std::sync::{Arc, Mutex};
use tauri::Manager;
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
        ])
        .events(collect_events![ytdlp::types::GlobalDownloadEvent]);

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
        .manage(Mutex::new(AppState {}))
        .setup(move |app| {
            builder.mount_events(app);
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            modules::logger::init(app_data_dir.clone());

            let db =
                ytdlp::db::Database::new(&app_data_dir).expect("Failed to initialize database");
            app.manage(Arc::new(db));

            // Initialize DownloadManager with max_concurrent from settings
            let settings =
                ytdlp::settings::get_settings_from_path(&app_data_dir).unwrap_or_default();
            let download_manager = Arc::new(ytdlp::download::DownloadManager::new(
                settings.max_concurrent,
            ));
            app.manage(download_manager);

            Ok(())
        })
        .invoke_handler(invoke_handler)
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                let manager = app_handle.state::<DownloadManagerState>();
                manager.cancel_all();
            }
        });
}
