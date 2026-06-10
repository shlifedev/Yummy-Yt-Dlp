use crate::modules::types::AppError;
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";

/// Localized ("Show Window", "Quit") labels based on the saved language setting.
/// The tray is built once at startup in Rust, so this reads the persisted language directly;
/// it covers users who explicitly picked a non-English language (who would notice English here).
fn tray_labels(app: &AppHandle) -> (&'static str, &'static str) {
    let lang = app
        .store(STORE_FILE)
        .ok()
        .and_then(|s| s.get("language"))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    match lang.as_str() {
        "ko" => ("창 보기", "종료"),
        "ja" => ("ウィンドウを表示", "終了"),
        "zh-CN" => ("显示窗口", "退出"),
        "zh-TW" => ("顯示視窗", "結束"),
        "fr" => ("Afficher la fenêtre", "Quitter"),
        "de" => ("Fenster anzeigen", "Beenden"),
        _ => ("Show Window", "Quit"),
    }
}

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let (show_label, quit_label) = tray_labels(app);
    let show = MenuItemBuilder::with_id("show", show_label).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", quit_label).build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("No default window icon configured")?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Yummy YT-DLP")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                // Stop new work, signal everything, then give the executor tasks a
                // bounded window to run kill_process_tree before exiting — otherwise
                // yt-dlp/ffmpeg survive the process as orphans. RunEvent::Exit waits
                // again, but by then both managers are already idle (harmless no-op).
                let manager = app
                    .state::<Arc<crate::ytdlp::download::DownloadManager>>()
                    .inner()
                    .clone();
                let scan_manager = app.state::<crate::ScanManagerState>().inner().clone();
                manager.shutdown();
                manager.cancel_all();
                scan_manager.cancel_current();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = tokio::join!(
                        manager.wait_until_idle(Duration::from_secs(3)),
                        scan_manager.wait_until_idle(Duration::from_secs(3)),
                    );
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}

pub fn get_minimize_to_tray_setting(app: &AppHandle) -> Option<bool> {
    let store = app.store(STORE_FILE).ok()?;
    store.get("minimizeToTray").and_then(|v| v.as_bool())
}

pub fn set_minimize_to_tray_setting(app: &AppHandle, value: bool) -> Result<(), AppError> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| AppError::Custom(e.to_string()))?;
    store.set(
        "minimizeToTray",
        serde_json::to_value(Some(value)).map_err(|e| AppError::Custom(e.to_string()))?,
    );
    store.save().map_err(|e| AppError::Custom(e.to_string()))?;
    Ok(())
}
