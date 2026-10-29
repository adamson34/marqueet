//! Settings backup: one file with the settings and team art, to keep or to
//! move to a new device. Secrets stay behind (the admin password, API keys,
//! feed tokens and Home Assistant's token), so the file is safe to keep
//! anywhere and a restore never lets anyone in.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use marqueet_core::settings::Settings;
use marqueet_core::team_art::{TeamArtMap, TeamPack};
use serde::{Deserialize, Serialize};

use crate::hub::Hub;
use crate::team_art::{MAX_PACK_BYTES, export_pack, pack_art};

/// Current version of the backup file format.
pub const BACKUP_VERSION: u32 = 1;

/// Largest backup file accepted: a full team pack plus the settings.
pub const MAX_BACKUP_BYTES: usize = MAX_PACK_BYTES + 1024 * 1024;

/// `{"marqueet_backup": 1, "saved": ..., "settings": {...}, "teams": {...}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub marqueet_backup: u32,
    /// When it was made, and by which version (for people reading the file).
    #[serde(default)]
    pub saved: Option<DateTime<Utc>>,
    #[serde(default)]
    pub made_by: String,
    pub settings: Settings,
    /// Team colors, logos, words and LED art, as a team pack.
    pub teams: TeamPack,
}

/// A backup of `settings` and `art`, made now.
pub fn make(settings: Settings, art: &TeamArtMap) -> Backup {
    Backup {
        marqueet_backup: BACKUP_VERSION,
        saved: Some(Utc::now()),
        made_by: format!("Marqueet {}", env!("CARGO_PKG_VERSION")),
        settings,
        teams: export_pack(art, &[]),
    }
}

/// Reads a backup file, saying what's wrong with one that won't do.
pub fn read(json: &[u8]) -> Result<Backup, String> {
    if json.len() > MAX_BACKUP_BYTES {
        return Err("that backup is too big (25 MB at most)".into());
    }
    let backup: Backup =
        serde_json::from_slice(json).map_err(|e| format!("that doesn't look like a Marqueet backup ({e})"))?;
    if backup.marqueet_backup != BACKUP_VERSION {
        return Err(format!(
            "that backup is version {}; this Marqueet reads version {BACKUP_VERSION}",
            backup.marqueet_backup
        ));
    }
    Ok(backup)
}

/// Puts a backup on this device: its settings replace the current ones and
/// its team art is added (over any art for the same team). Everything is
/// checked before anything changes. Slow for a big pack (PNG decoding), so
/// call it off the async threads.
pub fn restore(hub: &Arc<Hub>, backup: Backup) -> Result<(), String> {
    let art = pack_art(backup.teams, |_| None)?;
    hub.apply_settings(backup.settings)?;
    if !art.is_empty() {
        hub.set_team_art(art).map_err(|e| format!("the settings are restored, but not the team art: {e}"))?;
    }
    log::info!("restored a backup");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::Rgb;
    use marqueet_core::sports::{LeagueId, TeamColors, TeamId};
    use marqueet_core::team_art::TeamArt;

    fn art() -> TeamArtMap {
        let colors = TeamColors { primary: Rgb::RED, secondary: Some(Rgb::WHITE) };
        let a = TeamArt {
            label: "LA baseball".into(),
            colors: Some(colors),
            logo: None,
            words: Default::default(),
            art: None,
        };
        [(TeamId("espn:mlb:19".into()), a)].into()
    }

    #[test]
    fn backups_read_what_they_write() {
        let settings = Settings { leagues: vec![LeagueId::new("mlb")], ..Settings::default() };
        let backup = make(settings.clone(), &art());
        let json = serde_json::to_vec(&backup).unwrap();
        let back = read(&json).unwrap();
        assert_eq!(back, backup);
        assert_eq!(back.settings, settings);
        assert_eq!(pack_art(back.teams, |_| None).unwrap(), art().into_iter().collect::<Vec<_>>());
    }

    #[test]
    fn bad_backups_say_why() {
        assert!(read(b"{}").unwrap_err().contains("Marqueet backup"));
        assert!(read(br#"{"marqueet_team_pack":1,"teams":[]}"#).unwrap_err().contains("Marqueet backup"));
        let future = br#"{"marqueet_backup":9,"settings":{},"teams":{"marqueet_team_pack":1,"teams":[]}}"#;
        assert!(read(future).unwrap_err().contains("version 9"));
    }

    #[test]
    fn missing_settings_take_their_defaults() {
        let old =
            br#"{"marqueet_backup":1,"settings":{"leagues":["nfl"]},"teams":{"marqueet_team_pack":1,"teams":[]}}"#;
        let b = read(old).unwrap();
        assert_eq!(b.settings.leagues, [LeagueId::new("nfl")]);
        assert_eq!(b.settings.widgets, Settings::default().widgets);
    }

    #[test]
    fn no_secrets_in_the_file() {
        let json = serde_json::to_string(&make(Settings::default(), &art())).unwrap();
        for word in ["password", "token", "api_key", "mqk_"] {
            assert!(!json.contains(word), "{word} in {json}");
        }
    }
}
