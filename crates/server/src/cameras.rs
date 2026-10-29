//! Home cameras (ADR-0016): Home Assistant's token, the camera list, and
//! pictures, fetched only while a camera is on screen (the one up in a
//! camera widget, the next one just before its turn, and any camera a
//! takeover shows), decoded and shrunk here so displays get small images.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use marqueet_core::camera::{Camera, decode_snapshot, snapshot_key};
use marqueet_core::provider::{CameraProvider, CameraSource};
use marqueet_core::settings::{MAX_CAMERAS, Settings};
use marqueet_core::team_art::Image;
use tokio::sync::{Notify, watch};

use crate::settings_store::SettingsStore;

/// The latest picture of each camera, by snapshot key. Shared, so sending
/// one to several displays doesn't copy it, and a display connection can
/// tell which ones are new.
pub type SnapshotSet = BTreeMap<String, Arc<Image>>;

/// How long a takeover's camera keeps being fetched (the takeover lasts
/// about ten seconds; it may wait its turn behind another).
const TAKEOVER_SECS: u64 = 25;

/// Longest token accepted (Home Assistant's are a few hundred characters).
const MAX_TOKEN: usize = 4096;

/// How the cameras are doing, for the admin page.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct CameraStatus {
    pub last_picture: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

pub struct Cameras {
    provider: Mutex<Option<Arc<dyn CameraProvider>>>,
    db: Option<SettingsStore>,
    token: Mutex<Option<String>>,
    snapshots: watch::Sender<Arc<SnapshotSet>>,
    /// Cameras a takeover is showing, and until when to fetch them.
    takeovers: Mutex<HashMap<String, Instant>>,
    status: Mutex<CameraStatus>,
    /// Wakes the poller (settings changed, a takeover needs a camera).
    pub(crate) wake: Notify,
}

impl std::fmt::Debug for Cameras {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cameras").field("has_token", &self.has_token()).finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Cameras {
    pub fn new(db: Option<SettingsStore>) -> Cameras {
        let token = match db.as_ref().map(SettingsStore::ha_token).transpose() {
            Ok(t) => t.flatten(),
            Err(e) => {
                log::error!("couldn't load the Home Assistant token: {e}");
                None
            }
        };
        let (snapshots, _) = watch::channel(Arc::default());
        Cameras {
            provider: Mutex::default(),
            db,
            token: Mutex::new(token),
            snapshots,
            takeovers: Mutex::default(),
            status: Mutex::default(),
            wake: Notify::new(),
        }
    }

    pub fn set_provider(&self, provider: Option<Arc<dyn CameraProvider>>) {
        *lock(&self.provider) = provider;
    }

    pub fn has_token(&self) -> bool {
        lock(&self.token).is_some()
    }

    /// Saves Home Assistant's long-lived access token, or with `None`
    /// forgets it (and the pictures).
    pub fn set_token(&self, token: Option<&str>) -> Result<(), String> {
        let token = token.map(str::trim).filter(|t| !t.is_empty());
        if let Some(t) = token
            && (t.len() > MAX_TOKEN || !t.bytes().all(|b| b.is_ascii_graphic()))
        {
            return Err("that doesn't look like a Home Assistant access token".into());
        }
        if let Some(db) = &self.db {
            db.set_ha_token(token).map_err(|e| format!("couldn't save the token: {e}"))?;
        }
        *lock(&self.token) = token.map(str::to_owned);
        if token.is_none() {
            self.snapshots.send_replace(Arc::default());
            *lock(&self.status) = CameraStatus::default();
        }
        self.wake.notify_one();
        Ok(())
    }

    /// Where to fetch from, when Home Assistant is set up.
    pub fn source(&self, settings: &Settings) -> Option<CameraSource> {
        let url = settings.home_assistant.url.clone()?;
        let token = lock(&self.token).clone()?;
        Some(CameraSource { url, token })
    }

    /// The cameras Home Assistant offers now.
    pub async fn list(&self, settings: &Settings) -> Result<Vec<Camera>, String> {
        let provider = lock(&self.provider).clone().ok_or("cameras aren't available on this server")?;
        let source = self.source(settings).ok_or("set Home Assistant's address and a token first")?;
        let mut cameras = provider.cameras(&source).await.map_err(|e| e.to_string())?;
        cameras.truncate(MAX_CAMERAS);
        Ok(cameras)
    }

    pub fn subscribe(&self) -> watch::Receiver<Arc<SnapshotSet>> {
        self.snapshots.subscribe()
    }

    pub fn status(&self) -> CameraStatus {
        lock(&self.status).clone()
    }

    /// Fetches `camera_id` for a takeover for the next little while.
    pub fn show_in_takeover(&self, camera_id: &str) {
        lock(&self.takeovers).insert(camera_id.to_owned(), Instant::now() + Duration::from_secs(TAKEOVER_SECS));
        self.wake.notify_one();
    }

    /// Cameras takeovers are still showing.
    pub fn takeover_cameras(&self) -> Vec<String> {
        let now = Instant::now();
        let mut shown = lock(&self.takeovers);
        shown.retain(|_, until| *until > now);
        shown.keys().cloned().collect()
    }

    /// Fetches, decodes and publishes one camera's picture.
    pub async fn fetch(&self, source: &CameraSource, camera_id: &str) {
        let Some(provider) = lock(&self.provider).clone() else { return };
        let result = match provider.snapshot(source, camera_id).await {
            Ok(encoded) => tokio::task::spawn_blocking(move || decode_snapshot(&encoded.bytes))
                .await
                .unwrap_or_else(|e| Err(e.to_string())),
            Err(e) => Err(e.to_string()),
        };
        let mut status = lock(&self.status);
        match result {
            Ok(image) => {
                self.snapshots.send_modify(|set| {
                    Arc::make_mut(set).insert(snapshot_key(camera_id), Arc::new(image));
                });
                status.last_picture = Some(Utc::now());
                status.last_error = None;
            }
            Err(e) => {
                // Once per new problem, not every few seconds.
                if status.last_error.as_deref() != Some(e.as_str()) {
                    log::warn!("camera {camera_id}: {e}");
                }
                status.last_error = Some(e);
            }
        }
    }

    /// Lets go of pictures of cameras no longer on the list.
    pub fn retain(&self, cameras: &[Camera]) {
        let keep: Vec<String> = cameras.iter().map(|c| snapshot_key(&c.id)).collect();
        self.snapshots.send_if_modified(|set| {
            let before = set.len();
            Arc::make_mut(set).retain(|key, _| keep.contains(key));
            set.len() != before
        });
    }
}

/// The pictures in `now` that a display doesn't have yet (not in `sent`, or
/// newer), recording them as sent.
pub fn new_snapshots(sent: &mut SnapshotSet, now: &SnapshotSet) -> BTreeMap<String, Image> {
    let mut out = BTreeMap::new();
    for (key, image) in now {
        if sent.get(key).is_none_or(|old| !Arc::ptr_eq(old, image)) {
            out.insert(key.clone(), (**image).clone());
            sent.insert(key.clone(), Arc::clone(image));
        }
    }
    sent.retain(|key, _| now.contains_key(key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_new_pictures_are_sent_again() {
        let pic = |v: u8| Arc::new(Image::new(1, 1, vec![v, v, v, 255]).unwrap());
        let mut sent = SnapshotSet::new();
        let mut now: SnapshotSet = [("camera:camera.a".into(), pic(1)), ("camera:camera.b".into(), pic(2))].into();
        assert_eq!(new_snapshots(&mut sent, &now).len(), 2);
        assert!(new_snapshots(&mut sent, &now).is_empty(), "nothing new");
        now.insert("camera:camera.b".into(), pic(3));
        let fresh = new_snapshots(&mut sent, &now);
        assert_eq!(fresh.keys().collect::<Vec<_>>(), ["camera:camera.b"]);
        now.remove("camera:camera.a");
        assert!(new_snapshots(&mut sent, &now).is_empty());
        assert!(!sent.contains_key("camera:camera.a"), "forgotten");
    }

    #[test]
    fn tokens_are_checked_and_kept_apart() {
        let db = SettingsStore::in_memory().unwrap();
        let cams = Cameras::new(Some(db.clone()));
        assert!(!cams.has_token());
        assert!(cams.set_token(Some("has spaces in it")).is_err());
        cams.set_token(Some("  made-up.token  ")).unwrap();
        assert_eq!(db.ha_token().unwrap().as_deref(), Some("made-up.token"));
        assert!(Cameras::new(Some(db.clone())).has_token(), "kept across restarts");
        assert!(!format!("{cams:?}").contains("made-up"), "never in debug output");
        let mut settings = Settings::default();
        assert!(cams.source(&settings).is_none(), "no address yet");
        settings.home_assistant.url = Some("http://ha.local:8123".into());
        assert_eq!(cams.source(&settings).unwrap().token, "made-up.token");
        cams.set_token(None).unwrap();
        assert!(cams.source(&settings).is_none() && db.ha_token().unwrap().is_none());
    }

    #[test]
    fn takeover_cameras_expire() {
        let cams = Cameras::new(None);
        cams.show_in_takeover("camera.front_porch");
        assert_eq!(cams.takeover_cameras(), ["camera.front_porch"]);
        lock(&cams.takeovers).insert("camera.old".into(), Instant::now() - Duration::from_secs(1));
        assert_eq!(cams.takeover_cameras(), ["camera.front_porch"]);
    }
}
