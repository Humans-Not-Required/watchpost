use rusqlite::{Connection, Result};
use std::sync::Mutex;

pub struct Db {
    pub conn: Mutex<Connection>,
}

impl Db {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        let db = Db { conn: Mutex::new(conn) };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS monitors (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                url TEXT NOT NULL,
                method TEXT NOT NULL DEFAULT 'GET',
                interval_seconds INTEGER NOT NULL DEFAULT 300,
                timeout_ms INTEGER NOT NULL DEFAULT 10000,
                expected_status INTEGER NOT NULL DEFAULT 200,
                body_contains TEXT,
                headers TEXT,
                manage_key_hash TEXT NOT NULL,
                is_public INTEGER NOT NULL DEFAULT 0,
                is_paused INTEGER NOT NULL DEFAULT 0,
                current_status TEXT NOT NULL DEFAULT 'unknown',
                last_checked_at TEXT,
                confirmation_threshold INTEGER NOT NULL DEFAULT 2,
                consecutive_failures INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS heartbeats (
                id TEXT PRIMARY KEY,
                monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
                status TEXT NOT NULL,
                response_time_ms INTEGER NOT NULL,
                status_code INTEGER,
                error_message TEXT,
                checked_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_heartbeats_monitor ON heartbeats(monitor_id, checked_at DESC);

            CREATE TABLE IF NOT EXISTS incidents (
                id TEXT PRIMARY KEY,
                monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
                started_at TEXT NOT NULL DEFAULT (datetime('now')),
                resolved_at TEXT,
                cause TEXT NOT NULL,
                acknowledgement TEXT,
                acknowledged_by TEXT,
                acknowledged_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_incidents_monitor ON incidents(monitor_id, started_at DESC);

            CREATE TABLE IF NOT EXISTS notification_channels (
                id TEXT PRIMARY KEY,
                monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                channel_type TEXT NOT NULL,
                config TEXT NOT NULL,
                is_enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_notifications_monitor ON notification_channels(monitor_id);
        ")?;
        Ok(())
    }
}
