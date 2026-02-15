use crate::modules::types::AppError;
use std::sync::Arc;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItemBuilder::with_id("show", "Show Window").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("No default window icon configured")?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("Modern YT-DLP GUI")
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
                let manager = app.state::<Arc<crate::ytdlp::download::DownloadManager>>();
                manager.cancel_all();
                app.exit(0);
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
