//! Los comandos de Tauri y el glue de GTK/WebKit del cartel.
//!
//! Aparte de `lib.rs` a propósito: es la capa que habla con la ventana, el
//! webview y el bus, y no se puede ejercitar sin un display ni un WebKit reales.
//! Por eso vive en `commands.rs`, que el análisis de cobertura del taller excluye
//! —igual que los `.vue`—: medir cobertura de lo que ninguna prueba puede montar
//! sólo bloquea sin informar. Lo testeable (el acotado del tamaño) se queda en
//! `lib.rs`.

use std::sync::Arc;

use gtk::prelude::*;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::db::{self, Db};
use crate::{banner, banner_size, server, BANNER_MIN_HEIGHT, BANNER_WIDTH, BANNER_WIN};

/// Le pide al escritorio que traiga al frente la aplicación que mandó el aviso.
///
/// Hacer clic en una notificación tiene que mostrar la conversación, el correo
/// o lo que sea que avisó. Emitir `ActionInvoked` —que es lo que manda la
/// especificación— no alcanza: en Wayland la aplicación **no puede traerse sola
/// al frente**. El único que puede es el compositor, y de todo el escritorio el
/// único que le habla es vasak-desktop, por su método `PresentApp`.
///
/// Va **sin esperar respuesta** (ese servicio no contesta: una llamada normal se
/// colgaría hasta el plazo de D-Bus por cada clic) y en una tarea aparte, para
/// que un transporte tapado no demore marcar la notificación leída.
async fn traer_al_frente(app_name: String) {
    if app_name.is_empty() {
        return;
    }
    let app_name = app_name.as_str();
    let Some(conexion) = server::conexion() else {
        return;
    };

    let mensaje = zbus::message::Message::method("/org/vasak/os/Desktop", "PresentApp")
        .and_then(|b| b.destination("org.vasak.os.Desktop"))
        .and_then(|b| b.interface("org.vasak.os.Desktop"))
        .and_then(|b| b.with_flags(zbus::message::Flags::NoReplyExpected))
        // Sin arrancarlo si no está: un clic en una notificación no tiene por qué
        // levantar el escritorio.
        .and_then(|b| b.with_flags(zbus::message::Flags::NoAutoStart))
        .and_then(|b| b.build(&(app_name,)));

    match mensaje {
        Ok(mensaje) => {
            if let Err(e) = conexion.send(&mensaje).await {
                eprintln!("[flare] no se pudo pedir traer al frente «{app_name}»: {e}");
            }
        }
        Err(e) => eprintln!("[flare] no se pudo armar el pedido para «{app_name}»: {e}"),
    }
}

/// Lleva a cabo lo que prometía la notificación: la acción que la persona tocó.
#[tauri::command]
pub(crate) async fn activate_notification(
    app: AppHandle,
    notif_id: u32,
    action_key: String,
    id: Option<i64>,
    app_name: Option<String>,
) {
    server::emit_action(notif_id, &action_key).await;
    // Después de emitir la acción y no antes: si traer la ventana fallara, la
    // aplicación tiene que haberse enterado igual de que le tocaron el aviso.
    if let Some(nombre) = app_name {
        tauri::async_runtime::spawn(traer_al_frente(nombre));
    }
    if let Some(history_id) = id {
        mark_read(&app, history_id).await;
    }
    server::emit_dismissed(notif_id).await;
}

/// Cerrar el cartel a mano: la notificación queda leída y dada por cerrada.
#[tauri::command]
pub(crate) async fn dismiss_notification(app: AppHandle, id: i64, notif_id: u32) {
    mark_read(&app, id).await;
    server::emit_dismissed(notif_id).await;
}

/// Marca leída una notificación del historial, si la base pudo abrirse.
async fn mark_read(app: &AppHandle, history_id: i64) {
    let db = app
        .try_state::<Arc<Db>>()
        .map(|state| state.inner().clone());
    if let Some(db) = db {
        server::mark_read(&db, history_id).await;
    }
}

/// Ajusta la ventana del cartel al alto que ocupa la pila.
///
/// La ventana layer-shell no crece con su contenido: con un alto fijo, o
/// recortaba los carteles apilados o dejaba un rectángulo transparente que
/// igual se comía los clics del escritorio.
#[tauri::command]
pub(crate) fn resize_banner(app: AppHandle, width: i32, height: i32) -> Result<(), String> {
    let (width, height) = banner_size(width, height);
    app.run_on_main_thread(move || {
        BANNER_WIN.with(|w| {
            if let Some(win) = w.borrow().as_ref() {
                win.set_size_request(width, height);
                win.resize(width, height);
            }
        });
    })
    .map_err(|e| e.to_string())
}

/// Mapea la ventana de capa del cartel.
///
/// Separado del comando para que `banner.rs` lo pueda llamar al crearla: hasta
/// que la ventana se mapea, el webview no carga la página.
pub(crate) fn show_banner_window(app: &AppHandle) {
    let _ = app.run_on_main_thread(|| {
        BANNER_WIN.with(|w| {
            if let Some(win) = w.borrow().as_ref() {
                win.show_all();
            }
        });
    });
}

#[tauri::command]
pub(crate) fn show_banner(app: AppHandle) -> Result<(), String> {
    show_banner_window(&app);
    Ok(())
}

#[tauri::command]
pub(crate) fn hide_banner(app: AppHandle) -> Result<(), String> {
    // Se esconde, no se destruye: el webview queda residente y se reusa en la
    // próxima notificación. Destruirlo dejaba vivo el WebKitWebProcess
    // re-parentado —decenas colgados por sesión— y recrearlo cada vez costaba la
    // ventana de carga.
    app.run_on_main_thread(|| {
        BANNER_WIN.with(|w| {
            if let Some(win) = w.borrow().as_ref() {
                win.hide();
            }
        });
    })
    .map_err(|e| e.to_string())
}

/// Un error de la página, al mismo registro que el resto.
///
/// El webview no manda su consola a ningún lado: un fallo de JavaScript deja la
/// aplicación sin montar y el cartel sin aparecer, sin una línea en ningún
/// archivo. Esto es ese canal.
#[tauri::command]
pub(crate) fn trace_js(message: String) {
    eprintln!("[flare/js] {message}");
}

/// El frontend del cartel terminó de montar: se lleva lo que llegó mientras
/// cargaba. Ver el comentario de módulo de `banner.rs`.
#[tauri::command]
pub(crate) fn banner_ready() -> Vec<db::StoredNotification> {
    banner::take_pending()
}

/// Afina WebKit para el cartel: es estático, chico y efímero, no necesita los
/// cachés ni la GPU de un navegador. Baja el piso de memoria del webview.
fn tune_webkit(webview: &webkit2gtk::WebView) {
    use webkit2gtk::{
        CacheModel, HardwareAccelerationPolicy, SettingsExt, WebContextExt, WebViewExt,
    };
    if let Some(settings) = WebViewExt::settings(webview) {
        settings.set_enable_developer_extras(false);
        settings.set_enable_back_forward_navigation_gestures(false);
        settings.set_enable_webgl(false);
        // Un cartel estático y chico no gana nada con la GPU, y sin aceleración se
        // ahorra un proceso de GPU y los buffers DMABUF.
        settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
    }
    if let Some(context) = WebViewExt::web_context(webview) {
        // No navega páginas: con el caché mínimo alcanza.
        context.set_cache_model(CacheModel::DocumentViewer);
    }
}

/// Reparent the banner webview into a wlr-layer-shell window anchored
/// bottom-right, so the notification banner is positioned correctly on Wayland
/// (clients can't place ordinary toplevels). Mirrors the vasak-terminal overlay
/// approach.
pub(crate) fn setup_banner_layer(window: &WebviewWindow) {
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
    layer_win.set_default_size(BANNER_WIDTH, BANNER_MIN_HEIGHT);
    layer_win.set_size_request(BANNER_WIDTH, BANNER_MIN_HEIGHT);

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
                // Sin pedido de tamaño propio: el que manda es el de la ventana,
                // que cambia con la cantidad de carteles apilados.
                webview.set_size_request(-1, -1);
                if let Ok(wv) = webview.clone().dynamic_cast::<webkit2gtk::WebView>() {
                    tune_webkit(&wv);
                }
                layer_win.add(webview);
                gtk_win.hide();
                BANNER_WIN.with(|w| *w.borrow_mut() = Some(layer_win));
            } else {
                eprintln!("[flare] no webview child to reparent");
            }
        }
    }
}
