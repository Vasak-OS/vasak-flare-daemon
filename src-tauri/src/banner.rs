//! El cartel de notificaciones, creado cuando hace falta y desarmado después.
//!
//! La ventana del banner era la partida más cara de todo el demonio: declarada
//! en `tauri.conf.json`, Tauri la construía al arrancar y el WebKit completo
//! —proceso de red incluido— quedaba residente la sesión entera para dibujar
//! un cartel que aparece unos segundos por día. Medido en una sesión real:
//! ~250 MB entre el WebKitWebProcess y el WebKitNetworkProcess de una ventana
//! que ni siquiera estaba visible.
//!
//! La objeción obvia es la latencia: el webview se creaba al arrancar para que
//! el primer cartel saliera al instante. Pero una notificación no es un método
//! de entrada — llegar unos cientos de milisegundos tarde es invisible, y las
//! ráfagas (un chat activo) no pagan nada porque la ventana se queda viva
//! mientras haya movimiento. Sólo se desarma tras [`IDLE`] sin carteles.
//!
//! Lo que sí hay que resolver al crear tarde es la ventana de carga: entre que
//! la notificación llega y el frontend termina de montar pasan unos cientos de
//! milisegundos, y un `emit` en ese hueco se pierde en el aire. De ahí la cola:
//! [`deliver`] encola mientras el webview calienta, y el frontend al montar
//! llama a `banner_ready`, que la drena. El mismo mutex serializa ambos lados,
//! así que una notificación no puede caer justo en el medio.

use gtk::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::db::StoredNotification;

/// Cuánto silencio desarma el webview.
///
/// Corto de más y una conversación lenta paga la creación en cada mensaje;
/// largo de más y el ahorro no existe. Dos minutos cubre el ritmo de un chat
/// sin convertir el demonio en lo que era antes.
const IDLE_DEFAULT: Duration = Duration::from_secs(120);

/// El plazo real, con un escape para poder probarlo sin esperar dos minutos.
///
/// `VASAK_FLARE_IDLE_SECS` existe para eso: verificar que la memoria vuelve es
/// la mitad del sentido de este módulo, y sin un plazo corto la comprobación
/// tarda más que el ciclo de compilación.
fn idle() -> Duration {
    std::env::var("VASAK_FLARE_IDLE_SECS")
        .ok()
        .and_then(|valor| valor.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(IDLE_DEFAULT)
}

/// Si el frontend nunca avisó que está listo, algo salió mal al cargar.
/// Sin este plazo, un webview roto quedaría residente para siempre — que es
/// exactamente el estado del que venimos. Lo encolado queda en el historial.
const WARMUP_LIMIT: Duration = Duration::from_secs(300);

/// El frontend ya montó y drenó la cola: los `emit` llegan.
static READY: AtomicBool = AtomicBool::new(false);
/// Evita que dos notificaciones simultáneas construyan dos ventanas.
static CREATING: AtomicBool = AtomicBool::new(false);
/// Cambia con cada señal de vida; el desarme sólo procede si nadie la movió.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Notificaciones que llegaron mientras el webview calentaba.
static PENDING: Mutex<Vec<StoredNotification>> = Mutex::new(Vec::new());

/// Traza sólo cuando se pide, para poder seguir el ciclo de vida sin ensuciar
/// el journal de una sesión normal.
fn traza(mensaje: &str) {
    if std::env::var_os("VASAK_FLARE_TRACE").is_some() {
        eprintln!("[flare/banner] {mensaje}");
    }
}

fn touch() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

/// Hace llegar una notificación al cartel, exista o no todavía.
pub fn deliver(app: &AppHandle, stored: &StoredNotification) {
    touch();

    // El mismo candado decide si se emite o se encola: sin él, una
    // notificación podría caer entre el `READY = true` y el drenado, y no
    // estar ni en la cola ni en la pantalla.
    {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if READY.load(Ordering::SeqCst) {
            let _ = app.emit("notification://new", stored);
            return;
        }
        pending.push(stored.clone());
    }

    traza("notificación encolada; el cartel todavía no está listo");
    ensure_created(app);
}

/// Una notificación cerrada antes de mostrarse no tiene que mostrarse.
pub fn drop_pending(notif_id: u32) {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    pending.retain(|stored| stored.notif_id != notif_id);
}

/// El frontend terminó de montar: se lleva lo que se acumuló.
pub fn take_pending() -> Vec<StoredNotification> {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    READY.store(true, Ordering::SeqCst);
    traza(&format!("el cartel reclamó {} pendientes", pending.len()));
    std::mem::take(&mut *pending)
}

/// Marca actividad, para que el desarme por silencio no gane una carrera
/// contra un cartel que se está mostrando.
pub fn keep_alive() {
    touch();
}

/// La interfaz escondió el último cartel: si no pasa nada en [`IDLE`], se
/// desarma todo.
pub fn schedule_teardown(app: &AppHandle) {
    traza("se escondió el último cartel; arranca el reloj del desarme");
    let generation = touch();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(idle()).await;
        if GENERATION.load(Ordering::SeqCst) == generation {
            traza("silencio: se desarma el cartel");
            teardown(&app);
        } else {
            traza("hubo actividad: el cartel sigue vivo");
        }
    });
}

fn ensure_created(app: &AppHandle) {
    if app.get_webview_window("banner").is_some() {
        return;
    }
    if CREATING.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    // En Linux la ventana se construye en el hilo principal de GTK; desde el
    // executor de zbus esto explota o se cuelga según el día.
    let main = app.clone();
    let result = main.run_on_main_thread(move || {
        // Los mismos valores que declaraba `tauri.conf.json` cuando la ventana
        // era permanente; la única diferencia es cuándo se construye.
        let built = WebviewWindowBuilder::new(&app, "banner", WebviewUrl::default())
            .title("vasak-flare-daemon")
            .inner_size(420.0, 72.0)
            .decorations(false)
            .transparent(true)
            .skip_taskbar(true)
            .resizable(false)
            .always_on_top(true)
            .visible(false)
            .on_page_load(|_window, payload| {
                traza(&format!("página del cartel: {:?}", payload.event()));
            })
            .build();

        match built {
            Ok(window) => {
                crate::setup_banner_layer(&window);

                // Mapear la ventana ahora, no cuando el frontend lo pida.
                //
                // Un webview cuya ventana nunca se mapeó no carga la página, así
                // que Vue no montaba, no llamaba a `banner_ready` y nadie pedía
                // mostrar: la ventana esperaba al frontend y el frontend esperaba
                // a la ventana. La única razón por la que se llega acá es que hay
                // una notificación para mostrar, así que mapearla es además lo
                // que corresponde. Queda transparente y vacía los pocos
                // milisegundos que tarda la página en montar.
                crate::show_banner_window(&app);

                // Si el frontend nunca llega a `banner_ready`, esto desarma el
                // webview roto en vez de dejarlo residente para siempre.
                let watchdog = app.clone();
                let generation = GENERATION.load(Ordering::SeqCst);
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(WARMUP_LIMIT).await;
                    if !READY.load(Ordering::SeqCst)
                        && GENERATION.load(Ordering::SeqCst) == generation
                    {
                        eprintln!("[flare] el cartel nunca avisó que cargó; se desarma");
                        teardown(&watchdog);
                    }
                });
            }
            Err(error) => {
                eprintln!("[flare] no se pudo crear el cartel: {error}");
            }
        }
        CREATING.store(false, Ordering::SeqCst);
    });

    if result.is_err() {
        CREATING.store(false, Ordering::SeqCst);
    }
}

/// Desarma el webview y la ventana de capa, en ese orden.
///
/// Primero la ventana de Tauri: wry es el dueño del widget del webview y sabe
/// destruirlo aunque viva re-parentado en la ventana de capa. Recién después
/// se tira la cáscara de GTK, ya vacía. Al revés, GTK destruiría un widget que
/// wry cree suyo, y ese doble dueño es un segfault en el hilo principal.
fn teardown(app: &AppHandle) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        READY.store(false, Ordering::SeqCst);
        if let Some(window) = app.get_webview_window("banner") {
            let _ = window.destroy();
        }
        crate::BANNER_WIN.with(|w| {
            if let Some(win) = w.borrow_mut().take() {
                unsafe { win.destroy() };
            }
        });
        traza("cartel desarmado");
    });
}
