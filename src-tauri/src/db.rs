use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

/// A persisted notification. `id` is the stable history rowid used by the
/// desktop (GetUnread/MarkRead); `notif_id` is the ephemeral freedesktop id used
/// by the notification protocol (CloseNotification/NotificationClosed).
#[derive(Debug, Clone, Serialize)]
pub struct StoredNotification {
    pub id: i64,
    pub notif_id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub urgency: u8,
    pub actions: Vec<String>,
    pub created_at: i64,
    pub read: bool,
}

pub struct Db {
    conn: Mutex<Connection>,
}

fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("vasak-flare-daemon")
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS notifications (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        notif_id INTEGER NOT NULL,
        app_name TEXT NOT NULL,
        app_icon TEXT NOT NULL,
        summary TEXT NOT NULL,
        body TEXT NOT NULL,
        urgency INTEGER NOT NULL,
        actions TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        read INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS idx_notif_id ON notifications(notif_id);
    CREATE INDEX IF NOT EXISTS idx_read ON notifications(read);";

impl Db {
    pub fn new() -> rusqlite::Result<Self> {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let conn = Connection::open(dir.join("notifications.db"))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        Self::with_connection(conn)
    }

    /// Una base en memoria, para las pruebas: mismo esquema, sin tocar el disco.
    #[cfg(test)]
    pub fn in_memory() -> rusqlite::Result<Self> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn row(row: &rusqlite::Row) -> rusqlite::Result<StoredNotification> {
        let actions_json: String = row.get(7)?;
        Ok(StoredNotification {
            id: row.get(0)?,
            notif_id: row.get::<_, i64>(1)? as u32,
            app_name: row.get(2)?,
            app_icon: row.get(3)?,
            summary: row.get(4)?,
            body: row.get(5)?,
            urgency: row.get::<_, i64>(6)? as u8,
            actions: serde_json::from_str(&actions_json).unwrap_or_default(),
            created_at: row.get(8)?,
            read: row.get::<_, i64>(9)? != 0,
        })
    }

    pub fn insert(&self, n: &StoredNotification) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        let actions = serde_json::to_string(&n.actions).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "INSERT INTO notifications
             (notif_id, app_name, app_icon, summary, body, urgency, actions, created_at, read)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                n.notif_id as i64, n.app_name, n.app_icon, n.summary, n.body,
                n.urgency as i64, actions, n.created_at, n.read as i64,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Update the most recent row with the given `notif_id` (used for
    /// replaces_id). Returns whether a row was updated.
    pub fn update_by_notif_id(&self, n: &StoredNotification) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let actions = serde_json::to_string(&n.actions).unwrap_or_else(|_| "[]".to_string());
        let rows = conn.execute(
            "UPDATE notifications
             SET app_name = ?2, app_icon = ?3, summary = ?4, body = ?5,
                 urgency = ?6, actions = ?7, created_at = ?8, read = 0
             WHERE id = (SELECT id FROM notifications WHERE notif_id = ?1 ORDER BY id DESC LIMIT 1)",
            params![
                n.notif_id as i64, n.app_name, n.app_icon, n.summary, n.body,
                n.urgency as i64, actions, n.created_at,
            ],
        )?;
        Ok(rows > 0)
    }

    pub fn latest_id_for_notif(&self, notif_id: u32) -> rusqlite::Result<Option<i64>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM notifications WHERE notif_id = ?1 ORDER BY id DESC LIMIT 1",
            params![notif_id as i64],
            |r| r.get(0),
        )
        .optional()
    }

    pub fn list(&self, only_unread: bool, limit: i64) -> rusqlite::Result<Vec<StoredNotification>> {
        let conn = self.conn.lock().unwrap();
        let sql = if only_unread {
            "SELECT id,notif_id,app_name,app_icon,summary,body,urgency,actions,created_at,read
             FROM notifications WHERE read = 0 ORDER BY created_at DESC, id DESC LIMIT ?1"
        } else {
            "SELECT id,notif_id,app_name,app_icon,summary,body,urgency,actions,created_at,read
             FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?1"
        };
        let mut stmt = conn.prepare(sql)?;
        let items = stmt
            .query_map(params![limit], Self::row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub fn notif_id_for_history(&self, id: i64) -> rusqlite::Result<Option<u32>> {
        let conn = self.conn.lock().unwrap();
        let v: Option<i64> = conn
            .query_row("SELECT notif_id FROM notifications WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?;
        Ok(v.map(|x| x as u32))
    }

    pub fn unread_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM notifications WHERE read = 0", [], |r| r.get(0))
    }

    pub fn mark_read(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE notifications SET read = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn mark_all_read(&self) -> rusqlite::Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE notifications SET read = 1 WHERE read = 0", [])?;
        Ok(())
    }

    pub fn delete(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM notifications WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_all(&self) -> rusqlite::Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM notifications", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(notif_id: u32, summary: &str) -> StoredNotification {
        StoredNotification {
            id: 0,
            notif_id,
            app_name: "prueba".into(),
            app_icon: String::new(),
            summary: summary.into(),
            body: "cuerpo".into(),
            urgency: 1,
            actions: vec!["default".into(), "Abrir".into()],
            created_at: 1_700_000_000,
            read: false,
        }
    }

    /// Cerrar un cartel a mano marca leída sólo esa notificación: el contador de
    /// pendientes del escritorio no puede bajar de más.
    #[test]
    fn mark_read_solo_afecta_a_la_notificacion_pedida() {
        let db = Db::in_memory().unwrap();
        let primera = db.insert(&sample(1, "una")).unwrap();
        db.insert(&sample(2, "otra")).unwrap();
        assert_eq!(db.unread_count().unwrap(), 2);

        db.mark_read(primera).unwrap();

        assert_eq!(db.unread_count().unwrap(), 1);
        let pendientes = db.list(true, 10).unwrap();
        assert_eq!(pendientes.len(), 1);
        assert_eq!(pendientes[0].summary, "otra");
    }

    /// El botón de cerrar sólo tiene el id de historial que vino en el evento;
    /// desde ahí hay que poder llegar al id de freedesktop para avisar a la app.
    #[test]
    fn el_id_de_historial_lleva_al_id_de_freedesktop() {
        let db = Db::in_memory().unwrap();
        let history_id = db.insert(&sample(42, "una")).unwrap();

        assert_eq!(db.notif_id_for_history(history_id).unwrap(), Some(42));
        assert_eq!(db.notif_id_for_history(history_id + 999).unwrap(), None);
    }

    /// Marcar leída una notificación que ya no existe no puede fallar: el cartel
    /// puede sobrevivir a un borrado del historial.
    #[test]
    fn mark_read_de_algo_inexistente_no_falla() {
        let db = Db::in_memory().unwrap();
        assert!(db.mark_read(1234).is_ok());
    }

    /// Reemplazar por `replaces_id` reusa la fila y la vuelve a dejar pendiente:
    /// una notificación de progreso no debe llenar el historial de entradas.
    #[test]
    fn replaces_id_reusa_la_fila_y_la_deja_pendiente() {
        let db = Db::in_memory().unwrap();
        let history_id = db.insert(&sample(7, "primera")).unwrap();
        db.mark_read(history_id).unwrap();
        assert_eq!(db.unread_count().unwrap(), 0);

        assert!(db.update_by_notif_id(&sample(7, "segunda")).unwrap());

        let todas = db.list(false, 10).unwrap();
        assert_eq!(todas.len(), 1);
        assert_eq!(todas[0].summary, "segunda");
        assert_eq!(db.unread_count().unwrap(), 1);
        assert_eq!(db.latest_id_for_notif(7).unwrap(), Some(history_id));
    }

    /// Las acciones viajan como JSON en una columna de texto: si el ida y vuelta
    /// se rompiera, los botones del cartel desaparecerían del historial.
    #[test]
    fn las_acciones_sobreviven_al_ida_y_vuelta() {
        let db = Db::in_memory().unwrap();
        db.insert(&sample(1, "una")).unwrap();

        let guardada = &db.list(false, 10).unwrap()[0];
        assert_eq!(guardada.actions, vec!["default".to_string(), "Abrir".to_string()]);
    }
}
