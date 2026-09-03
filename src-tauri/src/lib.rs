mod banner;
mod db;
mod server;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use gtk::prelude::*;
use tauri::{AppHandle, Manager, WebviewWindow};

use db::Db;

/// Ancho de la pila de carteles. Tiene que coincidir con `BANNER_WIDTH` de la
/// interfaz: es el ancho que hace que los botones de acción no pisen el texto.
const BANNER_WIDTH: i32 = 420;

/// Alto de arranque: un cartel corto. Después lo ajusta `resize_banner`.
const BANNER_MIN_HEIGHT: i32 = 72;

/// Techos y pisos absolutos, por si la interfaz pide algo disparatado.
const BANNER_MIN_WIDTH: i32 = 240;
const BANNER_MAX_WIDTH: i32 = 800;
const BANNER_MAX_HEIGHT: i32 = 1200;

/// Dónde están los archivos de idioma.
///
/// El plugin sólo prueba rutas relativas al ejecutable y al directorio de
/// trabajo, y ninguna de esas existe cuando el binario está instalado en
/// /usr/bin: sin esto, un paquete instalado muestra las claves crudas.
fn locales_dir() -> Option<String> {
    let candidatos = [
        PathBuf::from("locales"),
        PathBuf::from("src-tauri/locales"),
        PathBuf::from("/usr/share/vasak-flare-daemon/locales"),
    ];

    candidatos
        .into_iter()
        .find(|ruta| ruta.is_dir())
        .map(|ruta| ruta.to_string_lossy().to_string())
}

/// El idioma del sistema, o español.
///
/// Las variables vacías no cuentan: `LC_ALL=""` junto a `LANG=en_US.UTF-8` es
/// una máquina en inglés, y quedarse con la vacía la dejaría en español.
fn default_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|nombre| std::env::var(nombre).ok())
        .find(|valor| !valor.trim().is_empty())
        .and_then(|valor| {
            valor
                .split(['_', '.', '@'])
                .next()
                .filter(|idioma| !idioma.is_empty())
                .map(str::to_string)
        })
        .filter(|idioma| idioma == "en" || idioma == "es")
        .unwrap_or_else(|| "es".to_string())
}

thread_local! {
    // The gtk-layer-shell window hosting the banner webview (main thread only).
    pub(crate) static BANNER_WIN: RefCell<Option<gtk::Window>> = const { RefCell::new(None) };
}

/// Le pide al escritorio que traiga al frente la aplicación que mandó el aviso.
///
/// Hacer clic en una notificación tiene que mostrar la conversación, el correo
/// o lo que sea que avisó. Emitir `ActionInvoked` —que es lo que manda la
/// especificación— no alcanza para eso: la aplicación se entera, pero en
/// Wayland **no puede traerse sola al frente**. El único que puede es el
/// compositor, y de todo el escritorio el único que le habla es vasak-desktop.
///
/// Por eso se le pide a él, por su método `PresentApp`. Si no está corriendo o
/// la aplicación no tiene ninguna ventana abierta, no pasa nada: la acción ya
/// se emitió igual, que es lo importante.
///
/// Va **sin esperar respuesta**. Ese servicio no contesta ninguno de sus
/// métodos, así que una llamada normal se quedaría colgada hasta que venciera
/// el plazo de D-Bus —veinticinco segundos— por cada clic.
///
/// Y se lanza en una tarea aparte, no encadenada al cierre del cartel: aunque
/// no espere respuesta, el envío puede demorarse si el transporte está tapado,
/// y de eso no puede depender que la notificación se marque leída y se cierre.
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
        // Y sin arrancarlo si no está: hacer clic en una notificación no tiene
        // por qué levantar el escritorio. Hoy ese nombre no es activable —no
        // hay ningún `.service` que lo declare—, pero el día que lo sea, esta
        // llamada no debería ser la que lo encienda.
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

/// Carry out what a notification promised: the action the person clicked.
///
/// The banner used to have a single behaviour —hide itself— so clicking a
/// notification that offered to open a page or bring an application to the
/// front did nothing. The identifier is the freedesktop one, which is what the
/// application that sent the notification knows about.
#[tauri::command]
async fn activate_notification(
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
    // Quien actúa sobre una notificación ya la vio: no tiene sentido que siga
    // contando como pendiente en el historial del escritorio.
    if let Some(history_id) = id {
        mark_read(&app, history_id).await;
    }
    // Y se da por cerrada: quien la mandó tiene que dejar de esperarla.
    server::emit_dismissed(notif_id).await;
}

/// Cerrar el cartel a mano: la notificación queda leída y dada por cerrada.
///
/// Antes el cartel sólo se podía dejar pasar (se iba a los cinco segundos), así
/// que no había ningún gesto que significara «ya la vi». El botón de cerrar es
/// ese gesto, y por eso además la marca leída.
#[tauri::command]
async fn dismiss_notification(app: AppHandle, id: i64, notif_id: u32) {
    mark_read(&app, id).await;
    server::emit_dismissed(notif_id).await;
}

/// Marca leída una notificación del historial, si la base pudo abrirse.
async fn mark_read(app: &AppHandle, history_id: i64) {
    let db = app.try_state::<Arc<Db>>().map(|state| state.inner().clone());
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
fn resize_banner(app: AppHandle, width: i32, height: i32) -> Result<(), String> {
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

/// Acota lo que pide la interfaz a algo que se pueda mostrar en una pantalla.
fn banner_size(width: i32, height: i32) -> (i32, i32) {
    (
        width.clamp(BANNER_MIN_WIDTH, BANNER_MAX_WIDTH),
        height.clamp(BANNER_MIN_HEIGHT, BANNER_MAX_HEIGHT),
    )
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
fn show_banner(app: AppHandle) -> Result<(), String> {
    banner::keep_alive();
    show_banner_window(&app);
    Ok(())
}

#[tauri::command]
fn hide_banner(app: AppHandle) -> Result<(), String> {
    let result = app
        .run_on_main_thread({
            let _app = app.clone();
            move || {
                BANNER_WIN.with(|w| {
                    if let Some(win) = w.borrow().as_ref() {
                        win.hide();
                    }
                });
            }
        })
        .map_err(|e| e.to_string());
    // Se escondió el último cartel: si no llega nada en un rato, el webview
    // entero se desarma y el demonio vuelve a no costar nada.
    banner::schedule_teardown(&app);
    result
}

/// Un error de la página, al mismo registro que el resto.
///
/// El webview no manda su consola a ningún lado: un fallo de JavaScript deja
/// la aplicación sin montar y el cartel sin aparecer, sin una línea en ningún
/// archivo. Esto es ese canal.
#[tauri::command]
fn trace_js(message: String) {
    eprintln!("[flare/js] {message}");
}

/// El frontend del cartel terminó de montar: se lleva lo que llegó mientras
/// cargaba. Ver el comentario de módulo de `banner.rs`.
#[tauri::command]
fn banner_ready() -> Vec<db::StoredNotification> {
    banner::take_pending()
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
        .plugin(tauri_plugin_i18n_vsk::init_with_path(
            Some(default_locale()),
            locales_dir(),
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_config_manager::init())
        .plugin(tauri_plugin_vicons::init())
        .invoke_handler(tauri::generate_handler![
            show_banner,
            hide_banner,
            banner_ready,
            trace_js,
            resize_banner,
            activate_notification,
            dismiss_notification
        ])
        .setup(|app| {
            // La ventana del cartel no existe todavía: se construye con la
            // primera notificación (banner.rs) y se desarma tras el silencio.
            // Arrancar sin ella es el punto — sin webview no hay WebKit
            // residente, y este proceso queda en el costo de un demonio Rust.
            match db::Db::new() {
                Ok(database) => {
                    let db = Arc::new(database);
                    // También queda a mano de los comandos: el botón de cerrar
                    // necesita marcar leída la notificación.
                    app.manage(db.clone());
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
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_app, event| {
            // Este proceso es un servicio, no una ventana.
            //
            // Tauri cierra la aplicación cuando se destruye la última ventana, y
            // desde que el cartel se crea y se desarma bajo demanda eso pasa
            // cada vez que hay silencio: el demonio se apagaba solo y las
            // notificaciones siguientes no encontraban a nadie en el bus.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El alto lo mide la interfaz, así que puede llegar cualquier cosa: un cero
    /// mientras se acomoda el texto, o una pila más alta que la pantalla.
    #[test]
    fn el_tamano_del_cartel_queda_dentro_de_lo_mostrable() {
        assert_eq!(banner_size(BANNER_WIDTH, 0), (BANNER_WIDTH, BANNER_MIN_HEIGHT));
        assert_eq!(banner_size(BANNER_WIDTH, -40), (BANNER_WIDTH, BANNER_MIN_HEIGHT));
        assert_eq!(
            banner_size(BANNER_WIDTH, 99_999),
            (BANNER_WIDTH, BANNER_MAX_HEIGHT)
        );
        assert_eq!(banner_size(10, 300), (BANNER_MIN_WIDTH, 300));
        assert_eq!(banner_size(4000, 300), (BANNER_MAX_WIDTH, 300));
    }

    /// Un alto razonable pasa tal cual: la pila tiene que poder crecer con los
    /// carteles que se apilan.
    #[test]
    fn un_alto_razonable_pasa_sin_tocar() {
        assert_eq!(banner_size(BANNER_WIDTH, 452), (BANNER_WIDTH, 452));
    }
}
