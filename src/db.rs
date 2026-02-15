use rusqlite::{Connection, Result, params};
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
                interval_seconds INTEGER NOT NULL DEFAULT 600,
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

        // Add tags column to monitors
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN tags TEXT NOT NULL DEFAULT '';").ok();

        // Add seq columns for cursor-based pagination
        conn.execute_batch("ALTER TABLE heartbeats ADD COLUMN seq INTEGER;").ok();
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_heartbeats_seq ON heartbeats(seq);").ok();
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_heartbeats_monitor_seq ON heartbeats(monitor_id, seq);").ok();

        conn.execute_batch("ALTER TABLE incidents ADD COLUMN seq INTEGER;").ok();
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_incidents_seq ON incidents(seq);").ok();
        conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_incidents_monitor_seq ON incidents(monitor_id, seq);").ok();

        // Add response_time_threshold_ms column to monitors
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN response_time_threshold_ms INTEGER;").ok();

        // Add follow_redirects column to monitors (default true — follow 301/302/etc.)
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN follow_redirects INTEGER NOT NULL DEFAULT 1;").ok();

        // Add group_name column to monitors (for organizing monitors into groups on status page)
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN group_name TEXT;").ok();

        // Add monitor_type column (http, tcp, dns) — default 'http' for backward compat
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN monitor_type TEXT NOT NULL DEFAULT 'http';").ok();

        // Add DNS check columns (for monitor_type='dns')
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN dns_record_type TEXT NOT NULL DEFAULT 'A';").ok();
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN dns_expected TEXT;").ok();

        // Add SLA tracking columns
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN sla_target REAL;").ok();
        conn.execute_batch("ALTER TABLE monitors ADD COLUMN sla_period_days INTEGER;").ok();

        // Settings table (key-value store for service-level config)
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
        ").ok();

        // Auto-generate admin key if not set
        let has_admin_key: bool = conn
            .query_row("SELECT COUNT(*) FROM settings WHERE key = 'admin_key'", [], |r| r.get::<_, i64>(0))
            .map(|c| c > 0)
            .unwrap_or(false);
        if !has_admin_key {
            let admin_key = crate::auth::generate_key();
            let admin_key_hash = crate::auth::hash_key(&admin_key);
            conn.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('admin_key_hash', ?1, datetime('now'))",
                rusqlite::params![admin_key_hash],
            ).ok();
            println!("🔑 Admin key (save this — shown once): {}", admin_key);
        }

        // Maintenance windows table
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS maintenance_windows (
                id TEXT PRIMARY KEY,
                monitor_id TEXT NOT NULL REFERENCES monitors(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                starts_at TEXT NOT NULL,
                ends_at TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_maintenance_monitor ON maintenance_windows(monitor_id);
            CREATE INDEX IF NOT EXISTS idx_maintenance_active ON maintenance_windows(starts_at, ends_at);
        ").ok();

        // Incident notes table
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS incident_notes (
                id TEXT PRIMARY KEY,
                incident_id TEXT NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
                content TEXT NOT NULL,
                author TEXT NOT NULL DEFAULT 'anonymous',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_incident_notes_incident ON incident_notes(incident_id, created_at ASC);
        ").ok();

        // Backfill seq for existing heartbeats
        let needs_hb_backfill: i64 = conn
            .query_row("SELECT COUNT(*) FROM heartbeats WHERE seq IS NULL", [], |r| r.get(0))
            .unwrap_or(0);
        if needs_hb_backfill > 0 {
            let mut stmt = conn.prepare("SELECT id FROM heartbeats WHERE seq IS NULL ORDER BY checked_at ASC, id ASC").unwrap();
            let ids: Vec<String> = stmt.query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect();
            let max_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM heartbeats", [], |r| r.get(0)).unwrap_or(0);
            for (i, id) in ids.iter().enumerate() {
                conn.execute("UPDATE heartbeats SET seq = ?1 WHERE id = ?2", params![max_seq + (i as i64) + 1, &id]).ok();
            }
        }

        // Backfill seq for existing incidents
        let needs_inc_backfill: i64 = conn
            .query_row("SELECT COUNT(*) FROM incidents WHERE seq IS NULL", [], |r| r.get(0))
            .unwrap_or(0);
        if needs_inc_backfill > 0 {
            let mut stmt = conn.prepare("SELECT id FROM incidents WHERE seq IS NULL ORDER BY started_at ASC, id ASC").unwrap();
            let ids: Vec<String> = stmt.query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect();
            let max_seq: i64 = conn.query_row("SELECT COALESCE(MAX(seq), 0) FROM incidents", [], |r| r.get(0)).unwrap_or(0);
            for (i, id) in ids.iter().enumerate() {
                conn.execute("UPDATE incidents SET seq = ?1 WHERE id = ?2", params![max_seq + (i as i64) + 1, &id]).ok();
            }
        }

        Ok(())
    }
}
