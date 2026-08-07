use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Emitter};
use zbus::object_server::SignalContext;
use zbus::zvariant::Value;
use zbus::{interface, Connection};

use crate::db::{Db, StoredNotification};

const NOTIF_PATH: &str = "/org/freedesktop/Notifications";
const VASAK_PATH: &str = "/org/vasak/Notifications";

/// Held so expiry tasks / actions can emit signals after the fact.
static CONN: OnceLock<Connection> = OnceLock::new();

pub struct FlareState {
    db: Arc<Db>,
    app: AppHandle,
    next_id: AtomicU32,
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
            let _ = NotificationServer::notification_closed(iface.signal_context(), id, reason).await;
        }
    }
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
    async fn action_invoked(ctxt: &SignalContext<'_>, id: u32, action_key: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn notification_closed(ctxt: &SignalContext<'_>, id: u32, reason: u32) -> zbus::Result<()>;

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

        // Update in place when replacing, so e.g. progress notifications don't
        // spawn a new history entry each time.
        let row_id = if replaces_id != 0 {
            match self.state.db.update_by_notif_id(&stored) {
                Ok(true) => self.state.db.latest_id_for_notif(id).ok().flatten().unwrap_or(0),
                _ => self.state.db.insert(&stored).unwrap_or(0),
            }
        } else {
            self.state.db.insert(&stored).unwrap_or(0)
        };
        stored.id = row_id;

        // Tell the UI to show a banner, and the desktop history to refresh.
        let _ = self.state.app.emit("notification://new", &stored);
        emit_changed().await;

        // Auto-close: default (-1) => 5s (never for critical); 0 => never; else ms.
        let expire_ms: Option<u64> = match expire_timeout {
            0 => None,
            t if t < 0 => (urgency < 2).then_some(5000),
            t => Some(t as u64),
        };
        if let Some(ms) = expire_ms {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                emit_closed(id, 1).await;
            });
        }

        id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_context)] ctxt: SignalContext<'_>,
    ) {
        let _ = Self::notification_closed(&ctxt, id, 3).await;
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
                    let _ =
                        NotificationServer::action_invoked(iface.signal_context(), notif_id, &action_key).await;
                }
            }
        }
    }
}

pub async fn start_server(db: Arc<Db>, app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(FlareState {
        db,
        app,
        next_id: AtomicU32::new(1),
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
        .at(NOTIF_PATH, NotificationServer { state: state.clone() })
        .await?;
    connection
        .object_server()
        .at(VASAK_PATH, VasakNotifications { state: state.clone() })
        .await?;
    let _ = connection.request_name("org.vasak.Notifications").await;

    // Keep the connection alive (and available to emit_closed / actions).
    let _ = CONN.set(connection);
    Ok(())
}
