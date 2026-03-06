mod commands;

use commands::AppState;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Arc::new(Mutex::new(AppState::default())))
        .setup(|app| {
            let quit = MenuItem::with_id(app, "quit", "Quit Vesa", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "Settings", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("Vesa - Software KVM")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        } else {
                            let _window = tauri::WebviewWindowBuilder::new(
                                app,
                                "main",
                                tauri::WebviewUrl::App("index.html".into()),
                            )
                            .title("Vesa Settings")
                            .inner_size(440.0, 520.0)
                            .resizable(false)
                            .build();
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_server,
            commands::stop_server,
            commands::start_client,
            commands::stop_client,
            commands::get_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
