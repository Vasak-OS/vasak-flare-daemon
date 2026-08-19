mod db;
mod server;

use std::cell::RefCell;
use std::sync::Arc;

use gtk::prelude::*;
use tauri::{AppHandle, Manager, WebviewWindow};

thread_local! {
    // The gtk-layer-shell window hosting the banner webview (main thread only).
    static BANNER_WIN: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
}

/// Carry out what a notification promised: the action the person clicked.
///
/// The banner used to have a single behaviour —hide itself— so clicking a
/// notification that offered to open a page or bring an application to the
/// front did nothing. The identifier is the freedesktop one, which is what the
/// application that sent the notification knows about.
#[tauri::command]
async fn activate_notification(notif_id: u32, action_key: String) {
    server::emit_action(notif_id, &action_key).await;
    // Y se da por cerrada: quien la mandó tiene que dejar de esperarla.
    server::emit_dismissed(notif_id).await;
}

#[tauri::command]
fn show_banner(app: AppHandle) -> Result<(), String> {
    app.run_on_main_thread(|| {
        BANNER_WIN.with(|w| {
            if let Some(win) = w.borrow().as_ref() {
                win.show_all();
            }
        });
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn hide_banner(app: AppHandle) -> Result<(), String> {
    app.run_on_main_thread(|| {
        BANNER_WIN.with(|w| {
            if let Some(win) = w.borrow().as_ref() {
                win.hide();
            }
        });
    })
    .map_err(|e| e.to_string())
}

/// Reparent the banner webview into a wlr-layer-shell window anchored
/// bottom-right, so the notification banner is positioned correctly on Wayland
/// (clients can't place ordinary toplevels). Mirrors the vasak-terminal overlay
/// approach.
fn setup_banner_layer(window: &WebviewWindow) {
    let gtk_win = match window.gtk_window() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[flare] could not get gtk window: {e}");
            return;
        }
    };

    use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

    let layer_win = gtk::Window::new(gtk::WindowType::Toplevel);
    layer_win.set_decorated(false);
    layer_win.set_default_size(380, 120);
    layer_win.set_size_request(380, 120);

    layer_win.init_layer_shell();
    layer_win.set_namespace("vasak-flare");
    layer_win.set_layer(Layer::Overlay);
    layer_win.set_anchor(Edge::Bottom, true);
    layer_win.set_anchor(Edge::Right, true);
    layer_win.set_layer_shell_margin(Edge::Bottom, 12);
    layer_win.set_layer_shell_margin(Edge::Right, 12);
    layer_win.set_keyboard_mode(KeyboardMode::None);

    // Transparent background so the rounded-corner CSS shows through.
    if let Some(screen) = gtk::gdk::Screen::default() {
        if let Some(visual) = screen.rgba_visual() {
            layer_win.set_visual(Some(&visual));
        }
        let provider = gtk::CssProvider::new();
        let _ = provider.load_from_data(b"window { background: transparent; }");
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    if let Some(child) = gtk_win.child() {
        if let Ok(container) = child.dynamic_cast::<gtk::Container>() {
            if let Some(webview) = container.children().first() {
                container.remove(webview);
                layer_win.add(webview);
                gtk_win.hide();
                BANNER_WIN.with(|w| *w.borrow_mut() = Some(layer_win));
            } else {
                eprintln!("[flare] no webview child to reparent");
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_config_manager::init())
        .plugin(tauri_plugin_vicons::init())
        .invoke_handler(tauri::generate_handler![show_banner, hide_banner, activate_notification])
        .setup(|app| {
            // Reparent the banner into a layer-shell overlay (kept hidden until
            // a notification arrives; the UI calls show_banner/hide_banner).
            if let Some(win) = app.get_webview_window("banner") {
                setup_banner_layer(&win);
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
