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

impl Db {
    pub fn new() -> rusqlite::Result<Self> {
        let dir = data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let conn = Connection::open(dir.join("notifications.db"))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notifications (
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
            CREATE INDEX IF NOT EXISTS idx_read ON notifications(read);",
        )?;
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
