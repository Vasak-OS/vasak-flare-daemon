//! No molestar (vasak-desktop#177).
//!
//! Con el modo puesto las notificaciones se siguen recibiendo y guardando en el
//! historial —el centro de control las muestra igual—, pero **no sale el
//! cartel**. Las de urgencia crítica (`urgency` = 2 en la especificación de
//! freedesktop) pasan siempre: una batería por agotarse no puede esperar a que
//! alguien se acuerde de mirar el centro.
//!
//! El estado vive acá, en el servidor, y no en el escritorio: el corte tiene que
//! pasar antes de crear la ventana del cartel, y la ventana la crea este
//! proceso. El escritorio sólo lo muestra y lo cambia por D-Bus.
//!
//! **Lo que cuesta:** cada notificación hace una lectura atómica en memoria
//! ([`DoNotDisturb::allows_banner`]); nada de disco en ese camino. El disco se
//! toca sólo al cambiar el modo, y sólo si de verdad cambió.
//!
//! **Por qué se guarda en la base de este demonio y no en `vasak.conf`:** el
//! único que lo escribe es este proceso, así que una tabla propia no compite
//! con nadie. `vasak.conf` lo escriben varias aplicaciones a la vez, y guardar
//! ahí con el gestor de configuración vuelve a aplicar el pack de iconos y las
//! fuentes por `gsettings` —subprocesos— en cada cambio, y despierta a todas
//! las aplicaciones que vigilan el archivo. Para un interruptor que el modo
//! juego prende y apaga cada vez que se abre un juego, eso es demasiado.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use zbus::interface;
use zbus::object_server::SignalContext;

use crate::db::Db;

/// La urgencia crítica de la especificación de notificaciones de freedesktop.
pub const URGENCY_CRITICAL: u8 = 2;

/// El estado del modo, en memoria.
#[derive(Debug, Default)]
pub struct DoNotDisturb {
    enabled: AtomicBool,
    /// Ordena los cambios entre sí, no las lecturas.
    ///
    /// Sin esto, dos `set` a la vez podían intercambiar el valor en un orden
    /// y escribirlo en el disco en el otro: la memoria decía «puesto» y la
    /// base «quitado», y la sesión siguiente arrancaba distinta. Las lecturas
    /// del camino de cada notificación no lo toman: siguen siendo una carga
    /// atómica.
    writes: Mutex<()>,
}

impl DoNotDisturb {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            writes: Mutex::new(()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        // Nada más depende de este valor: no hace falta ordenar otras lecturas
        // contra él, así que alcanza con la carga más barata.
        self.enabled.load(Ordering::Relaxed)
    }

    /// Si una notificación con esta urgencia muestra cartel.
    ///
    /// Es la única pregunta que se hace en el camino de cada notificación.
    pub fn allows_banner(&self, urgency: u8) -> bool {
        urgency >= URGENCY_CRITICAL || !self.is_enabled()
    }

    /// Pone el modo y devuelve el que había.
    ///
    /// `persist` se llama **sólo si cambió**: apagar lo que ya estaba apagado
    /// no escribe nada. El intercambio es atómico, así que dos pedidos a la vez
    /// no pueden devolver los dos el mismo «anterior».
    pub fn set(&self, enabled: bool, persist: impl FnOnce(bool)) -> bool {
        let _write = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let previous = self.enabled.swap(enabled, Ordering::AcqRel);
        if previous != enabled {
            persist(enabled);
        }
        previous
    }
}

/// El modo en el bus: `org.vasak.Notifications.DoNotDisturb`, en el mismo objeto
/// que el historial (`/org/vasak/Notifications`).
///
/// - `Enabled` (propiedad, sólo lectura): el estado. Cambia con
///   `PropertiesChanged`, así que quien lo muestra se entera sin preguntar.
/// - `SetEnabled(b) → b`: lo pone y devuelve el que había. El anterior viene en
///   la misma llamada para que el modo juego (vasak-desktop#181) pueda dejarlo
///   como estaba al salir sin una carrera entre leer y escribir.
pub struct DoNotDisturbInterface {
    pub state: Arc<DoNotDisturb>,
    pub db: Arc<Db>,
}

#[interface(name = "org.vasak.Notifications.DoNotDisturb")]
impl DoNotDisturbInterface {
    #[zbus(property)]
    async fn enabled(&self) -> bool {
        self.state.is_enabled()
    }

    async fn set_enabled(
        &self,
        enabled: bool,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) -> bool {
        let mut changed = false;
        let previous = self.state.set(enabled, |value| {
            changed = true;
            if let Err(e) = self.db.set_do_not_disturb(value) {
                // El modo cambia igual: perder el recuerdo entre sesiones es
                // mejor que ignorar el interruptor.
                eprintln!("[flare] no se pudo guardar «No molestar»: {e}");
            }
        });
        if changed {
            if let Err(e) = self.enabled_changed(&ctxt).await {
                eprintln!("[flare] no se pudo avisar el cambio de «No molestar»: {e}");
            }
        }
        previous
    }
}

