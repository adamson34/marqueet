//! User settings, stored by the server and edited from the admin page. Pure.

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};

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
    pub const ALL: [WidgetKind; 5] =
        [WidgetKind::GameOfTheDay, WidgetKind::Scores, WidgetKind::Standings, WidgetKind::Weather, WidgetKind::Fantasy];

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
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WidgetKind::GameOfTheDay => "Game of the day",
            WidgetKind::Scores => "Scores",
            WidgetKind::Standings => "Standings",
            WidgetKind::Weather => "Weather",
            WidgetKind::Fantasy => "Fantasy",
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
            WidgetKind::Standings => leagues("Automatic"),
            WidgetKind::Fantasy => s.fantasy.iter().map(|f| (f.key(), format!("{} ({})", f.team, f.league))).collect(),
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
}

impl Default for SpotlightSettings {
    fn default() -> Self {
        SpotlightSettings { auto: true, favorites: true, primetime: true, game: None }
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

/// Hours to blank the screen, local time, e.g. 23:00 to 07:00.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuietHours {
    pub from: NaiveTime,
    pub to: NaiveTime,
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
    /// One game filling the widget area (single live game, or picked).
    pub spotlight: SpotlightSettings,
}

/// Fantasy teams one device follows.
pub const MAX_FANTASY: usize = 4;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            leagues: DEFAULT_LEAGUES.iter().map(|l| LeagueId::new(*l)).collect(),
            favorites: Vec::new(),
            takeovers: TakeoverPolicy::All,
            widgets: vec![WidgetKind::GameOfTheDay.into(), WidgetKind::Scores.into()],
            display: DisplayConfig::default(),
            quiet_hours: None,
            time_zone: None,
            weather: WeatherSettings::default(),
            fantasy: Vec::new(),
            provider_logos: false,
            spotlight: SpotlightSettings::default(),
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
        // Options must still be one of the widget's choices.
        let settings = &self;
        let valid: Vec<bool> = settings
            .widgets
            .iter()
            .map(|w| {
                w.option.as_ref().is_none_or(|o| w.kind.choices(settings).iter().any(|(v, _)| v == o && !v.is_empty()))
            })
            .collect();
        for (w, ok) in self.widgets.iter_mut().zip(valid) {
            if !ok || w.option.as_deref() == Some("") {
                w.option = None;
            }
        }
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

    /// True when the ticker or a widget slot shows the weather (and so it
    /// should be fetched).
    pub fn wants_weather(&self) -> bool {
        self.weather.place.is_some()
            && (self.weather.ticker || self.widgets.iter().any(|w| w.kind == WidgetKind::Weather))
    }

    pub fn screen_off_at(&self, local: NaiveTime) -> bool {
        self.quiet_hours.is_some_and(|q| q.contains(local))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn quiet_hours_cross_midnight() {
        let q = QuietHours { from: t(23, 0), to: t(7, 0) };
        assert!(q.contains(t(23, 30)) && q.contains(t(2, 0)) && q.contains(t(6, 59)));
        assert!(!q.contains(t(7, 0)) && !q.contains(t(12, 0)) && !q.contains(t(22, 59)));
        let day = QuietHours { from: t(9, 0), to: t(17, 0) };
        assert!(day.contains(t(12, 0)) && !day.contains(t(18, 0)));
    }

    #[test]
    fn sanitize_normalizes_input() {
        let s = Settings {
            leagues: vec![LeagueId::new(" NFL "), LeagueId::new("nfl"), LeagueId::new("mlb"), LeagueId::new("")],
            favorites: vec![TeamId("espn:nfl:2".into()), TeamId("espn:nfl:2".into())],
            widgets: vec![WidgetKind::Scores.into()],
            quiet_hours: Some(QuietHours { from: t(1, 0), to: t(1, 0) }),
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
