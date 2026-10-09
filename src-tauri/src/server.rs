use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tauri::{AppHandle, Emitter};
use zbus::object_server::SignalContext;
use zbus::zvariant::Value;
use zbus::{interface, Connection};

use crate::db::{Db, StoredNotification};
use crate::do_not_disturb::{DoNotDisturb, DoNotDisturbInterface};

const NOTIF_PATH: &str = "/org/freedesktop/Notifications";
const VASAK_PATH: &str = "/org/vasak/Notifications";

/// Held so expiry tasks / actions can emit signals after the fact.
static CONN: OnceLock<Connection> = OnceLock::new();

pub struct FlareState {
    db: Arc<Db>,
    app: AppHandle,
    next_id: AtomicU32,
    /// «No molestar»: si las notificaciones no críticas muestran cartel.
    do_not_disturb: Arc<DoNotDisturb>,
    /// Qué entrega es la vigente para cada id de notificación.
    ///
    /// Una notificación reemplazada con `replaces_id` conserva el id, así que
    /// cada `notify` deja su propia tarea de expiración con el mismo id. Sin
    /// esto, la tarea vieja cerraba la notificación **nueva** al vencer su
    /// propio plazo: la saca de la cola y emite el cierre por una entrega que ya
    /// no existe. Cada entrega se queda con su número y sólo cierra si sigue
    /// siendo la vigente.
    revisiones: Mutex<HashMap<u32, u64>>,
}

struct NotificationServer {
    state: Arc<FlareState>,
}

struct VasakNotifications {
    state: Arc<FlareState>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve an icon name/path: explicit app_icon, then the image-path hint, then
/// an app-name heuristic.
fn resolve_icon(app_icon: String, app_name: &str, hints: &HashMap<String, Value<'_>>) -> String {
    if !app_icon.is_empty() {
        return app_icon;
    }
    for key in ["image-path", "image_path"] {
        if let Some(Value::Str(s)) = hints.get(key) {
            return s.to_string();
        }
    }
    let n = app_name.to_lowercase();
    if n.contains("chrome") {
        "google-chrome".to_string()
    } else if n.contains("telegram") {
        "telegram-desktop".to_string()
    } else {
        n
    }
}

/// Emit NotificationClosed for `id` (reason per the freedesktop spec: 1 expired,
/// 2 dismissed, 3 closed by call).
async fn emit_closed(id: u32, reason: u32) {
    if let Some(conn) = CONN.get() {
        if let Ok(iface) = conn
            .object_server()
            .interface::<_, NotificationServer>(NOTIF_PATH)
            .await
        {
            let _ =
                NotificationServer::notification_closed(iface.signal_context(), id, reason).await;
        }
    }
}

/// Tell the application that sent a notification that somebody acted on it.
///
/// This is the whole point of a notification that says "Reply" or opens a page:
/// without this signal the click does nothing at all, which is exactly what was
/// happening — the banner only knew how to hide itself.
/// La conexión al bus de sesión, para quien necesite hablarle a otro servicio.
///
/// Es la misma que ya sostiene el servidor de notificaciones: abrir una segunda
/// por cada mensaje suelto sería una conexión nueva al bus por cada clic.
pub fn conexion() -> Option<&'static Connection> {
    CONN.get()
}

pub async fn emit_action(notif_id: u32, action_key: &str) {
    if let Some(conn) = CONN.get() {
        if let Ok(iface) = conn
            .object_server()
            .interface::<_, NotificationServer>(NOTIF_PATH)
            .await
        {
            let _ =
                NotificationServer::action_invoked(iface.signal_context(), notif_id, action_key)
                    .await;
        }
    }
}

/// The notification is gone because somebody dealt with it.
///
/// Reason 2 is the user's doing, as opposed to the timeout (1). Applications
/// use this to stop tracking a notification they are still holding on to; a
/// server that never sends it leaves them waiting forever.
pub async fn emit_dismissed(notif_id: u32) {
    emit_closed(notif_id, 2).await;
}

/// Da por leída una notificación del historial y avisa a quien lo esté mirando.
///
/// Lo mismo que hace el método `MarkRead` de D-Bus, pero accesible desde los
/// comandos del cartel: cerrarlo a mano tiene que apagar el pendiente, si no el
/// escritorio sigue avisando por algo que la persona ya descartó.
pub async fn mark_read(db: &Db, history_id: i64) {
    if let Err(e) = db.mark_read(history_id) {
        eprintln!("[flare] could not mark notification {history_id} as read: {e}");
        return;
    }
    emit_changed().await;
}

/// Notify subscribers (the desktop history view) that the store changed.
async fn emit_changed() {
    if let Some(conn) = CONN.get() {
        if let Ok(iface) = conn
            .object_server()
            .interface::<_, VasakNotifications>(VASAK_PATH)
            .await
        {
            let _ = VasakNotifications::changed(iface.signal_context()).await;
        }
    }
}

/// Guarda la notificación en el historial y dice si lleva cartel.
///
/// Es lo que hace `Notify` antes de tocar la interfaz, aparte para poder
/// probarlo sin ventana ni bus: la notificación **siempre** se guarda —con
/// «No molestar» también, el centro de control la tiene que mostrar— y el
/// cartel depende del modo y de la urgencia.
fn accept(
    db: &Db,
    do_not_disturb: &DoNotDisturb,
    stored: &mut StoredNotification,
    replacing: bool,
) -> bool {
    // Update in place when replacing, so e.g. progress notifications don't
    // spawn a new history entry each time.
    stored.id = if replacing {
        match db.update_by_notif_id(stored) {
            Ok(true) => db
                .latest_id_for_notif(stored.notif_id)
                .ok()
                .flatten()
                .unwrap_or(0),
            _ => db.insert(stored).unwrap_or(0),
        }
    } else {
        db.insert(stored).unwrap_or(0)
    };

    do_not_disturb.allows_banner(stored.urgency)
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationServer {
    async fn get_capabilities(&self) -> Vec<String> {
        vec![
            "body".into(),
            "actions".into(),
            "persistence".into(),
            "icon-static".into(),
        ]
    }

    async fn get_server_information(&self) -> (String, String, String, String) {
        (
            "VasakOS Flare".into(),
            "VasakOS".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }

    #[zbus(signal)]
    async fn action_invoked(
        ctxt: &SignalContext<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notification_closed(
        ctxt: &SignalContext<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, Value<'_>>,
        expire_timeout: i32,
    ) -> u32 {
        let urgency = match hints.get("urgency") {
            Some(Value::U8(u)) => *u,
            _ => 1,
        };
        let app_icon = resolve_icon(app_icon, &app_name, &hints);

        // Honour replaces_id: reuse the given id, otherwise allocate a new one.
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            self.state.next_id.fetch_add(1, Ordering::SeqCst)
        };

        let mut stored = StoredNotification {
            id: 0,
            notif_id: id,
            app_name,
            app_icon,
            summary,
            body,
            urgency,
            actions,
            created_at: now_secs(),
            read: false,
        };

        // Tell the UI to show a banner, and the desktop history to refresh.
        // The banner webview may not exist yet — `deliver` creates it and
        // queues this until the frontend is listening.
        //
        // Con «No molestar» la notificación se guarda igual y el historial se
        // entera, pero el cartel no sale: el corte va acá, antes de crear la
        // ventana, para que con el modo puesto el webview ni se construya.
        if accept(
            &self.state.db,
            &self.state.do_not_disturb,
            &mut stored,
            replaces_id != 0,
        ) {
            crate::banner::deliver(&self.state.app, &stored);
        }
        emit_changed().await;

        // Número de esta entrega, para que su tarea de expiración no cierre una
        // notificación más nueva con el mismo id.
        let revision = {
            let mut revisiones = self
                .state
                .revisiones
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let actual = revisiones.entry(id).or_insert(0);
            *actual += 1;
            *actual
        };

        // Auto-close: default (-1) => 5s (never for critical); 0 => never; else ms.
        let expire_ms: Option<u64> = match expire_timeout {
            0 => None,
            t if t < 0 => (urgency < 2).then_some(5000),
            t => Some(t as u64),
        };
        if let Some(ms) = expire_ms {
            let app = self.state.app.clone();
            let estado = self.state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;

                // Si llegó un reemplazo, esta tarea ya no manda.
                let vigente = estado
                    .revisiones
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&id)
                    .copied()
                    .unwrap_or(0);
                if vigente != revision {
                    return;
                }
                // El mismo camino que el cierre explícito, y no sólo la señal de
                // D-Bus: si la notificación expiraba antes de que el frontend
                // reclamara la cola, `take_pending` la mostraba igual — un
                // cartel que ya había vencido. Y la interfaz nunca se enteraba
                // de que había que sacarlo.
                crate::banner::drop_pending(id);
                let _ = app.emit("notification://close", id);
                emit_closed(id, 1).await;
            });
        }

        id
    }

    async fn close_notification(&self, id: u32, #[zbus(signal_context)] ctxt: SignalContext<'_>) {
        let _ = Self::notification_closed(&ctxt, id, 3).await;
        // Closed before the warming webview could show it: out of the queue.
        crate::banner::drop_pending(id);
        let _ = self.state.app.emit("notification://close", id);
    }
}

#[interface(name = "org.vasak.Notifications")]
impl VasakNotifications {
    /// Emitted whenever the store changes (new notification, read/cleared), so
    /// the desktop history view can refresh without polling.
    #[zbus(signal)]
    async fn changed(ctxt: &SignalContext<'_>) -> zbus::Result<()>;

    /// Full history (newest first), JSON-encoded. `limit <= 0` uses a default cap.
    async fn get_all(&self, limit: i64) -> String {
        let limit = if limit <= 0 { 200 } else { limit };
        let items = self.state.db.list(false, limit).unwrap_or_default();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
    }

    /// Unread notifications, JSON-encoded.
    async fn get_unread(&self) -> String {
        let items = self.state.db.list(true, 200).unwrap_or_default();
        serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
    }

    async fn get_unread_count(&self) -> u32 {
        self.state.db.unread_count().unwrap_or(0) as u32
    }

    async fn mark_read(&self, id: i64) {
        let _ = self.state.db.mark_read(id);
        emit_changed().await;
    }

    async fn mark_all_read(&self) {
        let _ = self.state.db.mark_all_read();
        emit_changed().await;
    }

    async fn clear(&self, id: i64) {
        let _ = self.state.db.delete(id);
        emit_changed().await;
    }

    async fn clear_all(&self) {
        let _ = self.state.db.clear_all();
        emit_changed().await;
    }

    /// Invoke an action on a notification (by history id): translate to the
    /// notification's freedesktop id and emit ActionInvoked to the source app.
    async fn invoke_action(&self, id: i64, action_key: String) {
        if let Ok(Some(notif_id)) = self.state.db.notif_id_for_history(id) {
            if let Some(conn) = CONN.get() {
                if let Ok(iface) = conn
                    .object_server()
                    .interface::<_, NotificationServer>(NOTIF_PATH)
                    .await
                {
                    let _ = NotificationServer::action_invoked(
                        iface.signal_context(),
                        notif_id,
                        &action_key,
                    )
                    .await;
                }
            }

            // Y se da por cerrada: quien la mandó tiene que dejar de esperarla.
            emit_dismissed(notif_id).await;
        }
    }
}

pub async fn start_server(db: Arc<Db>, app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Lo que quedó de la sesión anterior. Se lee una vez; después manda la
    // memoria.
    let do_not_disturb = Arc::new(DoNotDisturb::new(db.do_not_disturb().unwrap_or_else(|e| {
        eprintln!("[flare] no se pudo leer «No molestar»; arranca apagado: {e}");
        false
    })));
    let state = Arc::new(FlareState {
        db: db.clone(),
        app,
        next_id: AtomicU32::new(1),
        do_not_disturb: do_not_disturb.clone(),
        revisiones: Mutex::new(HashMap::new()),
    });

    let connection = Connection::session().await?;

    use zbus::fdo::RequestNameFlags;
    connection
        .request_name_with_flags(
            "org.freedesktop.Notifications",
            RequestNameFlags::ReplaceExisting | RequestNameFlags::DoNotQueue,
        )
        .await?;

    connection
        .object_server()
        .at(
            NOTIF_PATH,
            NotificationServer {
                state: state.clone(),
            },
        )
        .await?;
    connection
        .object_server()
        .at(
            VASAK_PATH,
            VasakNotifications {
                state: state.clone(),
            },
        )
        .await?;
    connection
        .object_server()
        .at(
            VASAK_PATH,
            DoNotDisturbInterface {
                state: do_not_disturb,
                db,
            },
        )
        .await?;
    let _ = connection.request_name("org.vasak.Notifications").await;

    // Keep the connection alive (and available to emit_closed / actions).
    let _ = CONN.set(connection);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn una(notif_id: u32, urgency: u8) -> StoredNotification {
        StoredNotification {
            id: 0,
            notif_id,
            app_name: "Telegram".into(),
            app_icon: String::new(),
            summary: "Hola".into(),
            body: String::new(),
            urgency,
            actions: Vec::new(),
            created_at: 1_700_000_000,
            read: false,
        }
    }

    /// Con «No molestar» una notificación normal se guarda y no lleva cartel.
    #[test]
    fn con_no_molestar_la_normal_se_guarda_sin_cartel() {
        let db = Db::in_memory().unwrap();
        let dnd = DoNotDisturb::new(true);
        let mut normal = una(1, 1);

        assert!(!accept(&db, &dnd, &mut normal, false), "no lleva cartel");
        assert!(normal.id > 0, "tiene fila en el historial");
        assert_eq!(
            db.list(true, 10).unwrap().len(),
            1,
            "el centro la ve como pendiente"
        );
    }

    /// Una crítica pasa igual, y también queda guardada.
    #[test]
    fn con_no_molestar_la_critica_lleva_cartel() {
        let db = Db::in_memory().unwrap();
        let dnd = DoNotDisturb::new(true);
        let mut critica = una(2, 2);

        assert!(accept(&db, &dnd, &mut critica, false));
        assert_eq!(db.list(false, 10).unwrap().len(), 1);
    }

    /// Apagado el modo, los carteles vuelven, y un reemplazo con el modo puesto
    /// sigue actualizando la misma fila.
    #[test]
    fn al_apagarlo_vuelven_los_carteles() {
        let db = Db::in_memory().unwrap();
        let dnd = DoNotDisturb::new(true);
        assert!(!accept(&db, &dnd, &mut una(3, 1), false));
        assert!(
            !accept(&db, &dnd, &mut una(3, 1), true),
            "el reemplazo tampoco avisa"
        );
        assert_eq!(
            db.list(false, 10).unwrap().len(),
            1,
            "el reemplazo reusa la fila"
        );

        dnd.set(false, |_| {});
        assert!(accept(&db, &dnd, &mut una(4, 1), false));
    }

    #[test]
    fn el_icono_explicito_gana() {
        let hints = HashMap::new();
        assert_eq!(resolve_icon("firefox".into(), "Firefox", &hints), "firefox");
    }

    /// Muchas aplicaciones no mandan `app_icon` pero sí la imagen por hint: sin
    /// esto el cartel quedaba sin ícono.
    #[test]
    fn sin_icono_se_usa_la_imagen_del_hint() {
        let mut hints = HashMap::new();
        hints.insert("image-path".to_string(), Value::new("/tmp/foto.png"));
        assert_eq!(
            resolve_icon(String::new(), "Cualquiera", &hints),
            "/tmp/foto.png"
        );
    }

    #[test]
    fn ultimo_recurso_el_nombre_de_la_aplicacion() {
        let hints = HashMap::new();
        assert_eq!(
            resolve_icon(String::new(), "Telegram Desktop", &hints),
            "telegram-desktop"
        );
        assert_eq!(
            resolve_icon(String::new(), "Google Chrome", &hints),
            "google-chrome"
        );
        assert_eq!(
            resolve_icon(String::new(), "Resonance", &hints),
            "resonance"
        );
    }
}
