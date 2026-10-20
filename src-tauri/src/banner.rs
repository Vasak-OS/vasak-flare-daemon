//! El cartel de notificaciones: creado la primera vez que hace falta, y después
//! **residente y oculto**, reusado.
//!
//! La ventana del banner era la partida más cara del demonio: declarada en
//! `tauri.conf.json`, Tauri la construía al arrancar y el WebKit completo quedaba
//! residente la sesión entera para un cartel que aparece unos segundos por día.
//! Por eso se crea tarde, a demanda: una notificación no es un método de entrada,
//! unos cientos de milisegundos de más son invisibles.
//!
//! Lo que **no** se hace es desarmarlo al quedar en silencio. Se intentó —crear
//! al primer cartel y destruir tras un rato de inactividad— y cada ciclo
//! crear→destruir dejaba vivo el `WebKitWebProcess` del webview re-parentado en
//! la ventana de capa (wry/GTK no lo reapea): una sesión real juntaba decenas de
//! procesos colgados. Así que, una vez creado, el cartel se **esconde** entre
//! notificaciones y se **reusa**: un solo proceso, y además sale al instante. El
//! único que lo destruye es el vigilante de arranque, y sólo si el webview nunca
//! llegó a cargar (roto), para poder reintentar.
//!
//! Al crear tarde hay una ventana de carga: entre que la notificación llega y el
//! frontend termina de montar pasan unos cientos de milisegundos, y un `emit` en
//! ese hueco se perdería. De ahí la cola: [`deliver`] encola mientras el webview
//! calienta, y el frontend al montar llama a `banner_ready`, que la drena. El
//! mismo mutex serializa ambos lados, así que una notificación no cae en el medio.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::commands::ensure_created;
use crate::db::StoredNotification;

/// Si el frontend nunca avisó que está listo, algo salió mal al cargar.
/// Sin este plazo, un webview roto quedaría residente para siempre — que es
/// exactamente el estado del que venimos. Lo encolado queda en el historial.
pub(crate) const WARMUP_LIMIT: Duration = Duration::from_secs(300);

/// El frontend ya montó y drenó la cola: los `emit` llegan.
pub(crate) static READY: AtomicBool = AtomicBool::new(false);
/// Evita que dos notificaciones simultáneas construyan dos ventanas.
pub(crate) static CREATING: AtomicBool = AtomicBool::new(false);
/// Cambia **sólo** al construir un webview, para el vigilante de arranque: así
/// cada creación tiene su propio vigilante y una creación nueva invalida al
/// anterior, sin que una ráfaga de notificaciones lo haga darse por vencido y
/// deje un webview roto residente con la cola creciendo sin techo.
pub(crate) static CREATION: AtomicU64 = AtomicU64::new(0);

/// Notificaciones que llegaron mientras el webview calentaba.
static PENDING: Mutex<Vec<StoredNotification>> = Mutex::new(Vec::new());

/// Traza sólo cuando se pide, para poder seguir el ciclo de vida sin ensuciar
/// el journal de una sesión normal.
pub(crate) fn traza(mensaje: &str) {
    if std::env::var_os("VASAK_FLARE_TRACE").is_some() {
        eprintln!("[flare/banner] {mensaje}");
    }
}

/// Hace llegar una notificación al cartel, exista o no todavía.
pub fn deliver(app: &AppHandle, stored: &StoredNotification) {
    // El mismo candado decide si se emite o se encola: sin él, una
    // notificación podría caer entre el `READY = true` y el drenado, y no
    // estar ni en la cola ni en la pantalla.
    {
        let mut pending = PENDING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let mut pending = PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pending.retain(|stored| stored.notif_id != notif_id);
}

/// El frontend terminó de montar: se lleva lo que se acumuló.
pub fn take_pending() -> Vec<StoredNotification> {
    let mut pending = PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    READY.store(true, Ordering::SeqCst);
    traza(&format!("el cartel reclamó {} pendientes", pending.len()));
    std::mem::take(&mut *pending)
}

// `ensure_created` y `teardown` —el glue que construye, re-parentea y desarma el
// webview— viven en `commands.rs`, con el resto de la capa que no se puede
// ejercitar sin un display ni un WebKit reales (y que por eso el análisis de
// cobertura excluye). Acá queda sólo la lógica de la cola, que sí se puede leer
// sin montar nada.
