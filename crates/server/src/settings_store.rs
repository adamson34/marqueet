//! Settings persisted in SQLite: one row holding the settings as JSON, so new
//! fields get their defaults when an older database is read.

use std::path::Path;
use std::sync::Mutex;

use marqueet_core::settings::Settings;
use rusqlite::{Connection, OptionalExtension};

#[derive(Debug)]
pub struct SettingsStore {
    conn: Mutex<Connection>,
}

#[derive(Debug)]
pub enum StoreError {
    Sql(rusqlite::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Sql(e) => write!(f, "database: {e}"),
            StoreError::Json(e) => write!(f, "stored settings are unreadable: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}

impl SettingsStore {
    pub fn open(path: &Path) -> Result<SettingsStore, StoreError> {
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<SettingsStore, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<SettingsStore, StoreError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                json TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS feed_tokens (
                name TEXT PRIMARY KEY,
                token TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        Ok(SettingsStore { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The saved settings, or `None` on first run.
    pub fn load(&self) -> Result<Option<Settings>, StoreError> {
        let json: Option<String> =
            self.conn().query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0)).optional()?;
        json.map(|j| serde_json::from_str::<Settings>(&j).map(Settings::sanitized).map_err(StoreError::Json))
            .transpose()
    }

    /// Feed API tokens, by feed name. Kept out of the settings JSON so they
    /// never show up in `/api/settings`.
    pub fn feed_tokens(&self) -> Result<Vec<(String, String)>, StoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT name, token FROM feed_tokens ORDER BY name")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_feed_token(&self, name: &str, token: &str) -> Result<(), StoreError> {
        self.conn().execute(
            "INSERT INTO feed_tokens (name, token) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET token = excluded.token, created_at = datetime('now')",
            [name, token],
        )?;
        Ok(())
    }

    pub fn remove_feed_token(&self, name: &str) -> Result<(), StoreError> {
        self.conn().execute("DELETE FROM feed_tokens WHERE name = ?1", [name])?;
        Ok(())
    }

    pub fn save(&self, settings: &Settings) -> Result<(), StoreError> {
        let json = serde_json::to_string(settings).map_err(StoreError::Json)?;
        self.conn().execute(
            "INSERT INTO settings (id, json, updated_at) VALUES (1, ?1, datetime('now'))
             ON CONFLICT(id) DO UPDATE SET json = excluded.json, updated_at = excluded.updated_at",
            [json],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::settings::TakeoverPolicy;
    use marqueet_core::sports::TeamId;

    #[test]
    fn first_run_is_empty_then_round_trips() {
        let store = SettingsStore::in_memory().unwrap();
        assert!(store.load().unwrap().is_none());
        let s = Settings {
            favorites: vec![TeamId("espn:nfl:2".into())],
            takeovers: TakeoverPolicy::Favorites,
            ..Settings::default()
        };
        store.save(&s).unwrap();
        store.save(&s).unwrap(); // upsert, not a second row
        assert_eq!(store.load().unwrap(), Some(s));
    }

    #[test]
    fn persists_across_reopen() {
        let dir = std::env::temp_dir().join(format!("marqueet-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.db");
        let _ = std::fs::remove_file(&path);
        let s = Settings { takeovers: TakeoverPolicy::Off, ..Settings::default() };
        SettingsStore::open(&path).unwrap().save(&s).unwrap();
        assert_eq!(SettingsStore::open(&path).unwrap().load().unwrap().unwrap().takeovers, TakeoverPolicy::Off);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
