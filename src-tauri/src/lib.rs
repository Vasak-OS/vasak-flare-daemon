mod banner;
mod commands;
mod db;
mod do_not_disturb;
mod server;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;

use commands::{
    activate_notification, banner_ready, dismiss_notification, hide_banner, resize_banner,
    show_banner, trace_js,
};

/// Ancho de la pila de carteles. Tiene que coincidir con `BANNER_WIDTH` de la
/// interfaz: es el ancho que hace que los botones de acción no pisen el texto.
pub(crate) const BANNER_WIDTH: i32 = 420;

/// Alto de arranque: un cartel corto. Después lo ajusta `resize_banner`.
pub(crate) const BANNER_MIN_HEIGHT: i32 = 72;

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

/// Acota lo que pide la interfaz a algo que se pueda mostrar en una pantalla.
pub(crate) fn banner_size(width: i32, height: i32) -> (i32, i32) {
    (
        width.clamp(BANNER_MIN_WIDTH, BANNER_MAX_WIDTH),
        height.clamp(BANNER_MIN_HEIGHT, BANNER_MAX_HEIGHT),
    )
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
            // primera notificación (banner.rs). Arrancar sin ella es el punto —
            // sin webview no hay WebKit residente, y este proceso queda en el
            // costo de un demonio Rust.
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
            // Tauri cierra la aplicación cuando se destruye la última ventana.
            // Al arrancar no hay ninguna (el cartel se crea con la primera
            // notificación), así que sin esto el demonio se apagaría solo antes
            // del primer aviso y nadie quedaría en el bus.
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
        assert_eq!(
            banner_size(BANNER_WIDTH, 0),
            (BANNER_WIDTH, BANNER_MIN_HEIGHT)
        );
        assert_eq!(
            banner_size(BANNER_WIDTH, -40),
            (BANNER_WIDTH, BANNER_MIN_HEIGHT)
        );
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
