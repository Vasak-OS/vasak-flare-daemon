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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// El nombre de la interfaz en el bus, como lo escribe el escritorio: es
    /// contrato, y la prueba del bus lo usa tal cual.
    const INTERFACE: &str = "org.vasak.Notifications.DoNotDisturb";

    /// La prueba principal del modo: con «No molestar» una notificación normal
    /// (o baja) no tiene cartel, una crítica sí; apagado, todas lo tienen.
    #[test]
    fn con_no_molestar_solo_las_criticas_tienen_cartel() {
        let dnd = DoNotDisturb::new(true);
        assert!(!dnd.allows_banner(0), "una baja no muestra cartel");
        assert!(!dnd.allows_banner(1), "una normal no muestra cartel");
        assert!(
            dnd.allows_banner(URGENCY_CRITICAL),
            "una crítica pasa igual"
        );

        dnd.set(false, |_| {});
        for urgency in 0..=URGENCY_CRITICAL {
            assert!(
                dnd.allows_banner(urgency),
                "apagado, la urgencia {urgency} vuelve a avisar"
            );
        }
    }

    #[test]
    fn poner_el_modo_devuelve_el_anterior() {
        let dnd = DoNotDisturb::default();
        assert!(!dnd.set(true, |_| {}));
        assert!(dnd.set(true, |_| {}));
        assert!(dnd.set(false, |_| {}));
        assert!(!dnd.is_enabled());
    }

    /// Dos cambios a la vez: lo que queda en el disco es lo que quedó en
    /// memoria. A queda escribiendo despacio mientras B entra; sin el candado,
    /// B intercambia y escribe en el medio y A pisa el disco con un valor que
    /// la memoria ya no tiene.
    #[test]
    fn el_disco_y_la_memoria_no_se_separan() {
        let dnd = Arc::new(DoNotDisturb::default());
        let disk = Arc::new(Mutex::new(false));
        let (b_starting, wait_for_b) = std::sync::mpsc::channel::<()>();

        let a = {
            let dnd = dnd.clone();
            let disk = disk.clone();
            std::thread::spawn(move || {
                dnd.set(true, |value| {
                    wait_for_b.recv().unwrap();
                    // Tiempo de sobra para que B termine, si nada lo frena.
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    *disk.lock().unwrap() = value;
                });
            })
        };
        // Que A ya esté adentro de `set` antes de que B arranque.
        while !dnd.is_enabled() {
            std::thread::yield_now();
        }
        let b = {
            let dnd = dnd.clone();
            let disk = disk.clone();
            std::thread::spawn(move || {
                b_starting.send(()).unwrap();
                dnd.set(false, |value| *disk.lock().unwrap() = value);
            })
        };
        a.join().unwrap();
        b.join().unwrap();

        assert_eq!(*disk.lock().unwrap(), dnd.is_enabled());
    }

    /// El disco se toca sólo cuando el modo cambia de verdad.
    #[test]
    fn se_escribe_solo_si_cambia() {
        let dnd = DoNotDisturb::default();
        let writes = Cell::new(0);
        let persist = |_| writes.set(writes.get() + 1);

        dnd.set(false, persist);
        assert_eq!(writes.get(), 0, "apagar lo apagado no escribe");
        dnd.set(true, persist);
        dnd.set(true, persist);
        assert_eq!(writes.get(), 1, "prenderlo dos veces escribe una");
    }

    /// Un `dbus-daemon` propio para la prueba: nunca se toca el bus de la
    /// sesión, donde está el demonio instalado. Sin `dbus-daemon` en la máquina
    /// la prueba lo dice y no hace nada.
    struct PrivateBus {
        address: String,
        child: std::process::Child,
        dir: std::path::PathBuf,
    }

    impl PrivateBus {
        fn start() -> Option<Self> {
            use std::io::BufRead;
            let dir = std::env::temp_dir().join(format!("flare-dnd-bus-{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok()?;
            let mut child = std::process::Command::new("dbus-daemon")
                .args([
                    "--session",
                    "--nofork",
                    "--print-address=1",
                    &format!("--address=unix:dir={}", dir.display()),
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;
            let mut line = String::new();
            std::io::BufReader::new(child.stdout.take()?)
                .read_line(&mut line)
                .ok()?;
            Some(Self {
                address: line.trim().to_string(),
                child,
                dir,
            })
        }
    }

    impl Drop for PrivateBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// El contrato que usa el escritorio, por el bus: `SetEnabled` devuelve el
    /// anterior, `Enabled` lo refleja, el cambio llega por `PropertiesChanged`
    /// y queda guardado para la próxima sesión.
    #[tokio::test]
    async fn el_modo_se_pone_y_se_avisa_por_el_bus() {
        use futures_util::StreamExt;

        let Some(bus) = PrivateBus::start() else {
            eprintln!("sin dbus-daemon: no se prueba el contrato por el bus");
            return;
        };
        let db = Arc::new(Db::in_memory().unwrap());
        let _server = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .serve_at(
                "/org/vasak/Notifications",
                DoNotDisturbInterface {
                    state: Arc::new(DoNotDisturb::default()),
                    db: db.clone(),
                },
            )
            .unwrap()
            .name("org.vasak.Notifications")
            .unwrap()
            .build()
            .await
            .unwrap();
        let client = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let proxy = zbus::Proxy::new(
            &client,
            "org.vasak.Notifications",
            "/org/vasak/Notifications",
            INTERFACE,
        )
        .await
        .unwrap();
        let mut changes = proxy.receive_property_changed::<bool>("Enabled").await;

        let previous: bool = proxy.call("SetEnabled", &(true,)).await.unwrap();
        assert!(!previous, "antes estaba apagado");

        let change = tokio::time::timeout(std::time::Duration::from_secs(5), changes.next())
            .await
            .expect("el cambio llega por PropertiesChanged")
            .expect("el flujo sigue abierto");
        assert!(change.get().await.unwrap());
        assert!(proxy.get_property::<bool>("Enabled").await.unwrap());
        assert!(db.do_not_disturb().unwrap(), "queda guardado");

        let previous: bool = proxy.call("SetEnabled", &(false,)).await.unwrap();
        assert!(previous, "el anterior es el que había puesto");
    }
}
