//! Settings persisted in SQLite: one row holding the settings as JSON, so new
//! fields get their defaults when an older database is read.

use std::path::Path;
use std::sync::{Arc, Mutex};

use marqueet_core::settings::Settings;
use marqueet_core::sports::TeamId;
use std::collections::BTreeMap;

use marqueet_core::team_art::{Image, TeamArt, TeamArtMap};
use rusqlite::{Connection, OptionalExtension};

/// Cheap to clone: clones share one connection.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    conn: Arc<Mutex<Connection>>,
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
            CREATE TABLE IF NOT EXISTS admin (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                password_hash TEXT,
                reset_marker TEXT
            );
            CREATE TABLE IF NOT EXISTS feed_tokens (
                name TEXT PRIMARY KEY,
                token TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS provider_logos (
                team TEXT PRIMARY KEY,
                json TEXT NOT NULL,
                fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE TABLE IF NOT EXISTS team_art (
                team TEXT PRIMARY KEY,
                json TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        Ok(SettingsStore { conn: Arc::new(Mutex::new(conn)) })
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

    /// The admin password hash, if one was created at first boot.
    pub fn admin_password_hash(&self) -> Result<Option<String>, StoreError> {
        let hash: Option<Option<String>> =
            self.conn().query_row("SELECT password_hash FROM admin WHERE id = 1", [], |r| r.get(0)).optional()?;
        Ok(hash.flatten())
    }

    /// Stores (or with `None`, clears) the admin password hash.
    pub fn set_admin_password_hash(&self, hash: Option<&str>) -> Result<(), StoreError> {
        self.conn().execute(
            "INSERT INTO admin (id, password_hash) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET password_hash = excluded.password_hash",
            [hash],
        )?;
        Ok(())
    }

    /// The last password-reset file acted on (so it isn't acted on twice).
    pub fn reset_marker(&self) -> Result<Option<String>, StoreError> {
        let marker: Option<Option<String>> =
            self.conn().query_row("SELECT reset_marker FROM admin WHERE id = 1", [], |r| r.get(0)).optional()?;
        Ok(marker.flatten())
    }

    pub fn set_reset_marker(&self, marker: &str) -> Result<(), StoreError> {
        self.conn().execute(
            "INSERT INTO admin (id, reset_marker) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET reset_marker = excluded.reset_marker",
            [marker],
        )?;
        Ok(())
    }

    /// Feed API tokens, by feed name. Kept out of the settings JSON so they
    /// never show up in `/api/settings`.
    pub fn feed_tokens(&self) -> Result<Vec<(String, String)>, StoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT name, token FROM feed_tokens ORDER BY name")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Team colors and logos people added, kept apart from the settings so
    /// logos never ride along in `/api/settings`.
    pub fn team_art(&self) -> Result<TeamArtMap, StoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT team, json FROM team_art")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = TeamArtMap::new();
        for row in rows {
            let (team, json) = row?;
            match serde_json::from_str::<TeamArt>(&json) {
                Ok(art) => {
                    out.insert(TeamId(team), art);
                }
                Err(e) => log::warn!("skipping unreadable team art for {team}: {e}"),
            }
        }
        Ok(out)
    }

    pub fn set_team_art(&self, team: &TeamId, art: &TeamArt) -> Result<(), StoreError> {
        let json = serde_json::to_string(art).map_err(StoreError::Json)?;
        self.conn().execute(
            "INSERT INTO team_art (team, json) VALUES (?1, ?2)
             ON CONFLICT(team) DO UPDATE SET json = excluded.json, updated_at = datetime('now')",
            [team.0.as_str(), json.as_str()],
        )?;
        Ok(())
    }

    /// Logos downloaded from the scores provider (when the owner turned that
    /// on), so they're fetched once rather than on every start.
    pub fn provider_logos(&self) -> Result<BTreeMap<TeamId, Image>, StoreError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT team, json FROM provider_logos")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (team, json) = row?;
            if let Ok(image) =
                serde_json::from_str::<Image>(&json).map_err(|e| log::warn!("bad cached logo {team}: {e}"))
                && image.is_valid()
            {
                out.insert(TeamId(team), image);
            }
        }
        Ok(out)
    }

    pub fn set_provider_logo(&self, team: &TeamId, image: &Image) -> Result<(), StoreError> {
        let json = serde_json::to_string(image).map_err(StoreError::Json)?;
        self.conn().execute(
            "INSERT INTO provider_logos (team, json) VALUES (?1, ?2)
             ON CONFLICT(team) DO UPDATE SET json = excluded.json, fetched_at = datetime('now')",
            [team.0.as_str(), json.as_str()],
        )?;
        Ok(())
    }

    pub fn remove_team_art(&self, team: &TeamId) -> Result<(), StoreError> {
        self.conn().execute("DELETE FROM team_art WHERE team = ?1", [team.0.as_str()])?;
        Ok(())
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

    #[test]
    fn provider_logos_are_cached() {
        let db = SettingsStore::in_memory().unwrap();
        let team = TeamId("espn:nfl:12".into());
        let logo = Image::new(1, 1, vec![1, 2, 3, 4]).unwrap();
        db.set_provider_logo(&team, &logo).unwrap();
        db.set_provider_logo(&team, &logo).unwrap();
        assert_eq!(db.provider_logos().unwrap(), [(team, logo)].into());
    }

    #[test]
    fn team_art_is_saved_replaced_and_removed() {
        use marqueet_core::Rgb;
        use marqueet_core::sports::TeamColors;
        use marqueet_core::team_art::Image;
        let db = SettingsStore::in_memory().unwrap();
        assert!(db.team_art().unwrap().is_empty());
        let team = TeamId("espn:nfl:12".into());
        let mut art = TeamArt {
            label: "Somewhere".into(),
            colors: Some(TeamColors { primary: Rgb::RED, secondary: None }),
            logo: Image::new(1, 1, vec![1, 2, 3, 4]),
            words: Default::default(),
            art: Default::default(),
        };
        db.set_team_art(&team, &art).unwrap();
        assert_eq!(db.team_art().unwrap().get(&team), Some(&art));
        art.logo = None;
        db.set_team_art(&team, &art).unwrap();
        assert_eq!(db.team_art().unwrap().get(&team), Some(&art), "replaced");
        db.remove_team_art(&team).unwrap();
        assert!(db.team_art().unwrap().is_empty());
    }

    #[test]
    fn admin_password_and_reset_marker() {
        let db = SettingsStore::in_memory().unwrap();
        assert_eq!(db.admin_password_hash().unwrap(), None);
        db.set_admin_password_hash(Some("pbkdf2-sha256$1$00$00")).unwrap();
        assert_eq!(db.admin_password_hash().unwrap().as_deref(), Some("pbkdf2-sha256$1$00$00"));
        db.set_reset_marker("123").unwrap();
        assert_eq!(db.admin_password_hash().unwrap().as_deref(), Some("pbkdf2-sha256$1$00$00"), "kept");
        db.set_admin_password_hash(None).unwrap();
        assert_eq!(db.admin_password_hash().unwrap(), None);
        assert_eq!(db.reset_marker().unwrap().as_deref(), Some("123"));
        let clone = db.clone();
        clone.set_admin_password_hash(Some("x")).unwrap();
        assert_eq!(db.admin_password_hash().unwrap().as_deref(), Some("x"), "clones share the database");
    }
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
