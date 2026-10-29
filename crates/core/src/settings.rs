//! User settings, stored by the server and edited from the admin page. Pure.

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

use crate::camera::{Camera, MIN_REFRESH_SECS, is_camera_id};
use crate::config::DisplayConfig;
use crate::sports::ticker::league_label;
use crate::sports::{GameId, LeagueId, TeamId};
use crate::weather::{Place, Units};

pub const DEFAULT_LEAGUES: &[&str] = &["nfl", "ncaaf", "mlb", "nba", "wnba", "nhl", "mls", "epl"];

/// Which big plays take over the widget area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TakeoverPolicy {
    /// Every touchdown, home run and goal in the followed leagues.
    #[default]
    All,
    /// Only plays involving a favorite team; others just flash the ticker.
    Favorites,
    /// Never; everything just flashes the ticker.
    Off,
}

/// What a widget slot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    GameOfTheDay,
    Scores,
    Standings,
    Weather,
    Fantasy,
    /// A league's playoff bracket.
    Bracket,
    /// Home cameras, from Home Assistant (ADR-0016).
    Camera,
}

/// Home Assistant, for camera pictures (ADR-0016). The access token is a
/// secret and lives apart from the settings (see the server's store).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeAssistantSettings {
    /// `http://homeassistant.local:8123`; `None` until set.
    pub url: Option<String>,
    /// The cameras Home Assistant offered when last asked, by name.
    pub cameras: Vec<Camera>,
    /// Seconds between pictures of a camera on screen.
    pub refresh_secs: u16,
}

impl Default for HomeAssistantSettings {
    fn default() -> Self {
        HomeAssistantSettings { url: None, cameras: Vec::new(), refresh_secs: 3 }
    }
}

/// A Home Assistant address as typed, made tidy: `http` or `https`, no
/// trailing slash, no path, query or password. `None` if it isn't one.
pub fn clean_ha_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let ok = !rest.is_empty()
        && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._:[]".contains(&b))
        && !rest.starts_with(':');
    ok.then(|| url.to_owned())
}

/// A fantasy team to follow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FantasyLeague {
    /// Provider id, e.g. `sleeper`.
    pub provider: String,
    pub league_id: String,
    pub roster_id: u32,
    /// League and team names at the time it was added (for the admin page).
    pub league: String,
    pub team: String,
}

impl WidgetKind {
    /// Every kind, in menu order.
    pub const ALL: [WidgetKind; 7] = [
        WidgetKind::GameOfTheDay,
        WidgetKind::Scores,
        WidgetKind::Standings,
        WidgetKind::Bracket,
        WidgetKind::Weather,
        WidgetKind::Fantasy,
        WidgetKind::Camera,
    ];

    pub fn all() -> &'static [WidgetKind] {
        &Self::ALL
    }

    pub fn id(self) -> &'static str {
        match self {
            WidgetKind::GameOfTheDay => "game_of_the_day",
            WidgetKind::Scores => "scores",
            WidgetKind::Standings => "standings",
            WidgetKind::Weather => "weather",
            WidgetKind::Fantasy => "fantasy",
            WidgetKind::Bracket => "bracket",
            WidgetKind::Camera => "camera",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::GameOfTheDay => "Game of the day",
            WidgetKind::Scores => "Scores",
            WidgetKind::Standings => "Standings",
            WidgetKind::Weather => "Weather",
            WidgetKind::Fantasy => "Fantasy",
            WidgetKind::Bracket => "Playoff bracket",
            WidgetKind::Camera => "Camera",
        }
    }

    pub fn from_id(id: &str) -> Option<WidgetKind> {
        Self::all().iter().copied().find(|k| k.id() == id)
    }

    /// Choices for this widget's per-slot option, as (value, label); the
    /// first is the default. Empty when the widget has no option.
    pub fn choices(self, s: &Settings) -> Vec<(String, String)> {
        let leagues = |all: &str| {
            std::iter::once((String::new(), all.to_owned()))
                .chain(s.leagues.iter().map(|l| (l.as_str().to_owned(), league_label(l.as_str()))))
                .collect()
        };
        match self {
            WidgetKind::GameOfTheDay | WidgetKind::Scores => leagues("All leagues"),
            WidgetKind::Standings | WidgetKind::Bracket => leagues("Automatic"),
            WidgetKind::Fantasy => s.fantasy.iter().map(|f| (f.key(), format!("{} ({})", f.team, f.league))).collect(),
            WidgetKind::Camera => std::iter::once((String::new(), "Take turns".to_owned()))
                .chain(s.home_assistant.cameras.iter().map(|c| (c.id.clone(), c.name.clone())))
                .collect(),
            WidgetKind::Weather => Vec::new(),
        }
    }
}

/// A widget slot: what it shows and its option (a league, a fantasy team).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "SlotRepr")]
pub struct WidgetSlot {
    pub kind: WidgetKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
}

/// Slots were saved as bare kinds (`"scores"`) before they had options.
#[derive(Deserialize)]
#[serde(untagged)]
enum SlotRepr {
    Kind(WidgetKind),
    Full {
        kind: WidgetKind,
        #[serde(default)]
        option: Option<String>,
    },
}

impl From<SlotRepr> for WidgetSlot {
    fn from(r: SlotRepr) -> Self {
        match r {
            SlotRepr::Kind(kind) => WidgetSlot { kind, option: None },
            SlotRepr::Full { kind, option } => WidgetSlot { kind, option },
        }
    }
}

impl From<WidgetKind> for WidgetSlot {
    fn from(kind: WidgetKind) -> Self {
        WidgetSlot { kind, option: None }
    }
}

/// Widgets taking turns: more for each slot, and a set of their own for
/// when no game is live.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetRotation {
    /// After a slot's own widget, these take turns in it (by slot).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Vec<WidgetSlot>>,
    /// With no game live, these take the widget area in turn instead,
    /// filling the slots left to right. Empty: the slots stay as set.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub idle: Vec<WidgetSlot>,
    /// Seconds each turn lasts.
    pub every_secs: u16,
}

/// Widgets that can follow a slot's own.
pub const MAX_SLOT_TURNS: usize = 3;
/// Widgets in the set for when no game is live.
pub const MAX_IDLE_WIDGETS: usize = 6;
/// Turn lengths the admin page offers, in seconds.
pub const ROTATE_SECS: [u16; 4] = [10, 20, 30, 60];

impl Default for WidgetRotation {
    fn default() -> Self {
        WidgetRotation { slots: Vec::new(), idle: Vec::new(), every_secs: 20 }
    }
}

impl WidgetRotation {
    /// Every widget the rotation can show.
    pub fn kinds(&self) -> impl Iterator<Item = WidgetKind> + '_ {
        self.slots.iter().flatten().chain(&self.idle).map(|w| w.kind)
    }
}

impl FantasyLeague {
    /// "league_id:roster_id", a fantasy widget's option value.
    pub fn key(&self) -> String {
        format!("{}:{}", self.league_id, self.roster_id)
    }
}

/// Spotlight ("primetime"): one game fills the widget area.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpotlightSettings {
    /// Spotlight the game automatically when it's the only one live.
    pub auto: bool,
    /// Spotlight a favorite team's game while it's live, even with other
    /// games on.
    pub favorites: bool,
    /// Primetime: spotlight a football game while it's the only one live in
    /// its league (a Thursday night game), even with other sports on.
    pub primetime: bool,
    /// A game picked to watch, spotlighted whatever else is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<GameId>,
    /// The live play tracker in the spotlight: baseball's at-bat, football's
    /// drive.
    pub tracker: bool,
}

impl Default for SpotlightSettings {
    fn default() -> Self {
        SpotlightSettings { auto: true, favorites: true, primetime: true, game: None, tracker: true }
    }
}

/// Where and how to show the weather.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeatherSettings {
    /// `None` until set; nothing is fetched without it.
    pub place: Option<Place>,
    pub units: Units,
    /// Show the weather in the ticker (on by default once a place is set).
    pub ticker: bool,
    /// Official severe weather alerts for the place (US: National Weather
    /// Service), on the ticker and as takeovers. On by default.
    pub alerts: bool,
}

impl Default for WeatherSettings {
    fn default() -> Self {
        WeatherSettings { place: None, units: Units::default(), ticker: true, alerts: true }
    }
}

/// Night mode: hours to dim or blank the screen, local time, e.g. 23:00 to
/// 07:00.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    pub from: NaiveTime,
    pub to: NaiveTime,
    /// Dim the screen instead of turning it black.
    #[serde(default)]
    pub dim: bool,
}

impl QuietHours {
    /// True when `t` falls in the quiet window (which may cross midnight).
    pub fn contains(&self, t: NaiveTime) -> bool {
        if self.from <= self.to { t >= self.from && t < self.to } else { t >= self.from || t < self.to }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Leagues to follow, in ticker order.
    pub leagues: Vec<LeagueId>,
    /// Favorite teams (provider-namespaced ids, e.g. `espn:nfl:2`).
    pub favorites: Vec<TeamId>,
    pub takeovers: TakeoverPolicy,
    /// Widget slots, left to right.
    pub widgets: Vec<WidgetSlot>,
    /// Widgets taking turns in the slots.
    pub rotation: WidgetRotation,
    pub display: DisplayConfig,
    /// Blank the screen overnight.
    pub quiet_hours: Option<QuietHours>,
    /// IANA time zone for clocks, start times and quiet hours, e.g.
    /// `America/Chicago`. `None` uses the device's time zone.
    pub time_zone: Option<String>,
    pub weather: WeatherSettings,
    /// Fantasy teams to follow (at most [`MAX_FANTASY`]).
    pub fantasy: Vec<FantasyLeague>,
    /// Show team logos from the scores provider (ESPN), downloaded for the
    /// teams in today's games. Off unless the owner turns it on (ADR-0013).
    pub provider_logos: bool,
    /// Show betting lines (spread and over/under) with upcoming games and in
    /// the spotlight, for information. Off by default.
    pub show_odds: bool,
    /// One game filling the widget area (single live game, or picked).
    pub spotlight: SpotlightSettings,
    /// Home Assistant, for cameras.
    pub home_assistant: HomeAssistantSettings,
}

/// Cameras kept from Home Assistant's list.
pub const MAX_CAMERAS: usize = 16;

/// Fantasy teams one device follows.
pub const MAX_FANTASY: usize = 4;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            leagues: DEFAULT_LEAGUES.iter().map(|l| LeagueId::new(*l)).collect(),
            favorites: Vec::new(),
            takeovers: TakeoverPolicy::All,
            widgets: vec![WidgetKind::GameOfTheDay.into(), WidgetKind::Scores.into()],
            rotation: WidgetRotation::default(),
            display: DisplayConfig::default(),
            quiet_hours: None,
            time_zone: None,
            weather: WeatherSettings::default(),
            fantasy: Vec::new(),
            provider_logos: false,
            show_odds: false,
            spotlight: SpotlightSettings::default(),
            home_assistant: HomeAssistantSettings::default(),
        }
    }
}

impl Settings {
    /// Normalizes user input: lowercase, deduplicated leagues (at least one),
    /// deduplicated favorites, one widget per layout slot, clamped display values.
    pub fn sanitized(mut self) -> Settings {
        let mut leagues: Vec<LeagueId> = Vec::new();
        for l in self.leagues {
            let l = LeagueId::new(l.as_str().trim().to_lowercase());
            if !l.as_str().is_empty() && !leagues.contains(&l) {
                leagues.push(l);
            }
        }
        if leagues.is_empty() {
            leagues = Settings::default().leagues;
        }
        self.leagues = leagues;
        let mut favorites = Vec::new();
        for f in self.favorites {
            if !f.0.is_empty() && !favorites.contains(&f) {
                favorites.push(f);
            }
        }
        self.favorites = favorites;
        self.display = self.display.sanitized();
        // One widget per slot of the layout; new slots get sensible fills.
        const FILL: [WidgetKind; 3] = [WidgetKind::GameOfTheDay, WidgetKind::Scores, WidgetKind::Standings];
        let slots = self.display.widget_layout.slots();
        self.widgets.truncate(slots);
        while self.widgets.len() < slots {
            self.widgets.push(FILL[self.widgets.len() % FILL.len()].into());
        }
        // Turns: one list per slot, a few widgets each.
        let mut rotation = std::mem::take(&mut self.rotation);
        rotation.slots.truncate(slots);
        for turns in &mut rotation.slots {
            turns.truncate(MAX_SLOT_TURNS);
        }
        while rotation.slots.last().is_some_and(Vec::is_empty) {
            rotation.slots.pop();
        }
        rotation.idle.truncate(MAX_IDLE_WIDGETS);
        if rotation.every_secs == 0 {
            rotation.every_secs = WidgetRotation::default().every_secs;
        }
        rotation.every_secs = rotation.every_secs.clamp(5, 600);

        // Home Assistant: a tidy address, real camera ids, a sane refresh.
        let ha = &mut self.home_assistant;
        ha.url = ha.url.as_deref().and_then(clean_ha_url);
        let mut cameras: Vec<Camera> = Vec::new();
        for c in std::mem::take(&mut ha.cameras) {
            if is_camera_id(&c.id) && !cameras.iter().any(|d| d.id == c.id) {
                cameras.push(c);
            }
        }
        cameras.truncate(MAX_CAMERAS);
        ha.cameras = cameras;
        ha.refresh_secs = ha.refresh_secs.clamp(MIN_REFRESH_SECS, 60);
        // Options must still be one of the widget's choices.
        let mut widgets = std::mem::take(&mut self.widgets);
        for w in widgets.iter_mut().chain(rotation.slots.iter_mut().flatten()).chain(&mut rotation.idle) {
            let ok =
                w.option.as_ref().is_none_or(|o| w.kind.choices(&self).iter().any(|(v, _)| v == o && !v.is_empty()));
            if !ok {
                w.option = None;
            }
        }
        self.widgets = widgets;
        self.rotation = rotation;
        if self.quiet_hours.is_some_and(|q| q.from == q.to) {
            self.quiet_hours = None;
        }
        let mut fantasy: Vec<FantasyLeague> = Vec::new();
        for f in self.fantasy {
            if !fantasy.iter().any(|g| g.league_id == f.league_id && g.roster_id == f.roster_id) {
                fantasy.push(f);
            }
        }
        fantasy.truncate(MAX_FANTASY);
        self.fantasy = fantasy;
        self.time_zone = self.time_zone.map(|z| z.trim().to_owned()).filter(|z| !z.is_empty());
        self
    }

    pub fn is_favorite(&self, team: &TeamId) -> bool {
        self.favorites.contains(team)
    }

    /// The cameras a widget can show right now (and so should be fetched):
    /// a camera slot's own camera, or every camera for one taking turns.
    /// Empty without a Home Assistant address.
    pub fn wanted_cameras(&self) -> Vec<&Camera> {
        let ha = &self.home_assistant;
        if ha.url.is_none() {
            return Vec::new();
        }
        let mut wanted: Vec<&Camera> = Vec::new();
        for slot in self.widgets.iter().filter(|w| w.kind == WidgetKind::Camera) {
            for c in &ha.cameras {
                if slot.option.as_ref().is_none_or(|o| *o == c.id) && !wanted.iter().any(|w| w.id == c.id) {
                    wanted.push(c);
                }
            }
        }
        wanted
    }

    /// True when the ticker or a widget slot shows the weather (and so it
    /// should be fetched).
    pub fn wants_weather(&self) -> bool {
        self.weather.place.is_some()
            && (self.weather.ticker
                || self.widgets.iter().map(|w| w.kind).chain(self.rotation.kinds()).any(|k| k == WidgetKind::Weather))
    }

    /// True when night mode turns the screen black at `local`.
    pub fn screen_off_at(&self, local: NaiveTime) -> bool {
        self.quiet_hours.is_some_and(|q| !q.dim && q.contains(local))
    }

    /// True when night mode dims the screen at `local`.
    pub fn dimmed_at(&self, local: NaiveTime) -> bool {
        self.quiet_hours.is_some_and(|q| q.dim && q.contains(local))
    }

    /// These settings with a JSON merge patch (RFC 7396) applied: objects
    /// merge, `null` resets a field to its default, anything else replaces
    /// it. For the assistant API, which changes a few fields at a time.
    pub fn patched(&self, patch: &serde_json::Value) -> Result<Settings, String> {
        if !patch.is_object() {
            return Err("the patch has to be a JSON object".into());
        }
        let mut doc = serde_json::to_value(self).map_err(|e| e.to_string())?;
        merge_patch(&mut doc, patch);
        serde_json::from_value::<Settings>(doc).map(Settings::sanitized).map_err(|e| format!("settings: {e}"))
    }
}

fn merge_patch(doc: &mut serde_json::Value, patch: &serde_json::Value) {
    let serde_json::Value::Object(fields) = patch else {
        *doc = patch.clone();
        return;
    };
    if !doc.is_object() {
        *doc = serde_json::Value::Object(serde_json::Map::new());
    }
    if let serde_json::Value::Object(d) = doc {
        for (k, v) in fields {
            if v.is_null() {
                d.remove(k);
            } else {
                merge_patch(d.entry(k.clone()).or_insert(serde_json::Value::Null), v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn patches_merge_fields_and_reset_with_null() {
        let base = Settings { show_odds: true, time_zone: Some("America/Chicago".into()), ..Settings::default() };
        let patched =
            base.patched(&serde_json::json!({ "leagues": ["NFL", "epl"], "time_zone": null, "display": {} })).unwrap();
        assert_eq!(patched.leagues, vec![LeagueId::new("nfl"), LeagueId::new("epl")], "sanitized");
        assert_eq!(patched.time_zone, None, "null resets");
        assert!(patched.show_odds, "untouched fields stay");
        assert_eq!(patched.display, base.display, "an empty object changes nothing");
        assert!(base.patched(&serde_json::json!({ "show_odds": "yes" })).is_err());
        assert!(base.patched(&serde_json::json!(["nfl"])).is_err());
    }

    #[test]
    fn quiet_hours_cross_midnight() {
        let q = QuietHours { from: t(23, 0), to: t(7, 0), dim: false };
        assert!(q.contains(t(23, 30)) && q.contains(t(2, 0)) && q.contains(t(6, 59)));
        assert!(!q.contains(t(7, 0)) && !q.contains(t(12, 0)) && !q.contains(t(22, 59)));
        let day = QuietHours { from: t(9, 0), to: t(17, 0), dim: false };
        assert!(day.contains(t(12, 0)) && !day.contains(t(18, 0)));
    }

    #[test]
    fn night_mode_dims_or_blanks() {
        let night = |dim| Settings {
            quiet_hours: Some(QuietHours { from: t(23, 0), to: t(7, 0), dim }),
            ..Settings::default()
        };
        let (dim, black) = (night(true), night(false));
        assert!(dim.dimmed_at(t(2, 0)) && !dim.screen_off_at(t(2, 0)));
        assert!(black.screen_off_at(t(2, 0)) && !black.dimmed_at(t(2, 0)));
        assert!(!dim.dimmed_at(t(12, 0)) && !black.screen_off_at(t(12, 0)), "daytime");
        let old: QuietHours = serde_json::from_str(r#"{"from":"23:00:00","to":"07:00:00"}"#).unwrap();
        assert!(!old.dim, "saved before dimming existed: black, as it was");
    }

    #[test]
    fn sanitize_normalizes_input() {
        let s = Settings {
            leagues: vec![LeagueId::new(" NFL "), LeagueId::new("nfl"), LeagueId::new("mlb"), LeagueId::new("")],
            favorites: vec![TeamId("espn:nfl:2".into()), TeamId("espn:nfl:2".into())],
            widgets: vec![WidgetKind::Scores.into()],
            quiet_hours: Some(QuietHours { from: t(1, 0), to: t(1, 0), dim: false }),
            display: DisplayConfig { ticker_rows: 1000, ..Default::default() },
            time_zone: Some("  ".into()),
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.leagues, vec![LeagueId::new("nfl"), LeagueId::new("mlb")]);
        assert_eq!(s.favorites.len(), 1);
        let kinds = |s: &Settings| s.widgets.iter().map(|w| w.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&s), vec![WidgetKind::Scores, WidgetKind::Scores]);
        let mut three = s.clone();
        three.display.widget_layout = crate::config::WidgetLayout::Three;
        assert_eq!(kinds(&three.sanitized()), vec![WidgetKind::Scores, WidgetKind::Scores, WidgetKind::Standings]);
        let mut one = s.clone();
        one.display.widget_layout = crate::config::WidgetLayout::Single;
        assert_eq!(kinds(&one.sanitized()), vec![WidgetKind::Scores]);
        assert_eq!(s.quiet_hours, None, "empty window");
        assert_eq!(s.display.ticker_rows, 48);
        assert_eq!(s.time_zone, None, "blank means the device's zone");
        assert_eq!(Settings { leagues: vec![], ..Default::default() }.sanitized().leagues.len(), 8);
    }

    #[test]
    fn rotation_is_kept_to_the_layout_and_limits() {
        use WidgetKind::*;
        let mlb: WidgetSlot = WidgetSlot { kind: Standings, option: Some("mlb".into()) };
        let s = Settings {
            leagues: vec![LeagueId::new("nfl")],
            rotation: WidgetRotation {
                slots: vec![vec![Weather.into(); 5], vec![mlb.clone()], vec![Scores.into()]],
                idle: vec![Fantasy.into(); 9],
                every_secs: 0,
            },
            ..Settings::default()
        }
        .sanitized();
        let r = &s.rotation;
        assert_eq!(r.slots.len(), 2, "two slots in the default layout");
        assert_eq!((r.slots[0].len(), r.idle.len()), (MAX_SLOT_TURNS, MAX_IDLE_WIDGETS));
        assert_eq!(r.slots[1][0].option, None, "mlb isn't followed");
        assert_eq!(r.every_secs, 20);
        assert!(Settings::default().rotation.slots.is_empty());

        // Settings saved before rotation load with none.
        let old: Settings = serde_json::from_str(r#"{"widgets":["scores"]}"#).unwrap();
        assert_eq!(old.rotation, WidgetRotation::default());

        // A rotating weather widget gets the weather fetched.
        let mut w = Settings::default();
        w.weather.place =
            Some(crate::weather::Place { name: "Home".into(), latitude: 0.0, longitude: 0.0, time_zone: None });
        w.weather.ticker = false;
        assert!(!w.wants_weather());
        w.rotation.idle = vec![Weather.into()];
        assert!(w.wants_weather());
    }

    #[test]
    fn slots_load_old_and_new_forms_and_drop_stale_options() {
        let s: Settings = serde_json::from_str(
            r#"{"leagues":["nfl","mlb"],"widgets":["scores",{"kind":"standings","option":"mlb"}]}"#,
        )
        .unwrap();
        assert_eq!(s.widgets[0], WidgetSlot { kind: WidgetKind::Scores, option: None });
        assert_eq!(s.widgets[1].option.as_deref(), Some("mlb"));
        let json = serde_json::to_string(&s.widgets).unwrap();
        assert_eq!(json, r#"[{"kind":"scores"},{"kind":"standings","option":"mlb"}]"#);
        let dropped = Settings { leagues: vec![LeagueId::new("nfl")], ..s }.sanitized();
        assert_eq!(dropped.widgets[1].option, None, "mlb isn't followed any more");
        let choices = WidgetKind::Standings.choices(&dropped);
        assert_eq!(choices[0], (String::new(), "Automatic".into()));
        assert_eq!(choices[1], ("nfl".into(), "NFL".into()));
        assert!(WidgetKind::Weather.choices(&dropped).is_empty());
        assert_eq!(WidgetKind::from_id("fantasy"), Some(WidgetKind::Fantasy));
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings =
            serde_json::from_str(r#"{"favorites":["espn:nfl:2"],"quiet_hours":{"from":"23:00:00","to":"07:00:00"}}"#)
                .unwrap();
        assert_eq!(s.leagues.len(), 8);
        assert!(s.is_favorite(&TeamId("espn:nfl:2".into())));
        assert!(s.screen_off_at(t(3, 0)));
        assert_eq!(s.takeovers, TakeoverPolicy::All);
    }
}
