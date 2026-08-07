mod db;
mod server;

use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_config_manager::init())
        .plugin(tauri_plugin_vicons::init())
        .setup(|app| {
            // Headless daemon: keep the banner window hidden until a
            // notification arrives (the UI shows/hides it on the
            // notification:// events emitted by the server).
            if let Some(win) = app.get_webview_window("banner") {
                let _ = win.hide();
            }

            match db::Db::new() {
                Ok(database) => {
                    let db = Arc::new(database);
                    let app_handle = app.handle().clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = server::start_server(db, app_handle).await {
                            eprintln!("[flare] notification server failed to start: {e}");
                        }
                    });
                }
                Err(e) => eprintln!("[flare] could not open notifications DB: {e}"),
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
