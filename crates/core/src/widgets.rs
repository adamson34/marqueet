//! Widget view models: ready-to-draw content for the widget area.
//!
//! The server builds these from games (and later standings, fantasy,
//! weather) plus settings, so the display only draws; it never interprets
//! sports data (ADR-0003, ADR-0007). Pure.

use chrono::{DateTime, Datelike, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::color::{Rgb, led_team_color};
use crate::settings::{Settings, WidgetKind};
use crate::sports::standings::{self, Standings, StandingsGroup};
use crate::sports::ticker::league_label;
use crate::sports::{Competitor, Game, GameStatus, InningHalf, Situation, Sport, TeamId};
use crate::weather::{Condition, Weather, describe};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WidgetView {
    GameOfTheDay(GameOfTheDay),
    Scores(Scores),
    Standings(StandingsView),
    Weather(WeatherView),
    /// Nothing to show (e.g. no games today).
    Empty {
        title: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Side {
    pub abbr: String,
    /// "Away · 2-1"
    pub detail: String,
    pub score: Option<u16>,
    pub color: Rgb,
    /// Dimmed after a loss.
    pub lost: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameOfTheDay {
    /// "NFL  |  Q3 4:31" or "NFL  |  SUN 1:00PM".
    pub status: String,
    pub live: bool,
    pub away: Side,
    pub home: Side,
    /// Situation chips: "BUF ball", "2nd & 6", "KC 14".
    pub chips: Vec<String>,
    /// Line score column headers: "Q1".."Q4", innings, "T".
    pub columns: Vec<String>,
    /// (team abbreviation, cells) for away then home; last cell is the total.
    pub rows: Vec<(String, Vec<String>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Live,
    Break,
    Final,
    Upcoming,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreRow {
    pub league: String,
    /// "LAD 3" / "MUN"
    pub away: String,
    pub home: String,
    /// "TOP 7", "2nd 11:05", "FINAL", "4:30 PM"
    pub status: String,
    pub tone: Tone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scores {
    pub title: String,
    pub rows: Vec<ScoreRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandingsLine {
    /// Position in the group, from 1.
    pub rank: u32,
    pub team: String,
    pub cells: Vec<String>,
    /// A favorite (or a team in the featured game).
    pub highlight: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandingsView {
    pub title: String,
    /// "NFL  |  AFC EAST", or just "EPL" for a single table.
    pub group: String,
    pub columns: Vec<String>,
    /// The whole group, best first; the display shows as many as fit.
    pub rows: Vec<StandingsLine>,
    /// Row to keep on screen when not all fit.
    pub focus: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastDay {
    /// "TODAY", "THU".
    pub name: String,
    pub high: String,
    pub low: String,
    pub condition: Condition,
    /// "60%" when rain or snow is likely enough to mention.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precipitation: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeatherView {
    pub title: String,
    pub place: String,
    /// "72°"
    pub temperature: String,
    /// "Partly cloudy"
    pub summary: String,
    pub condition: Condition,
    pub day: bool,
    /// "Feels 70°", "Wind 9 mph", "Humidity 55%"
    pub details: Vec<String>,
    pub days: Vec<ForecastDay>,
}

fn degrees(t: f32) -> String {
    format!("{}°", t.round() as i32)
}

pub fn weather_view(w: &Weather) -> WeatherView {
    let c = &w.current;
    let today = w.days.first().map(|d| d.date);
    WeatherView {
        title: "WEATHER".into(),
        place: w.place.name.clone(),
        temperature: degrees(c.temperature),
        summary: describe(c.code).into(),
        condition: Condition::from_wmo(c.code),
        day: c.is_day,
        details: vec![
            format!("Feels {}", degrees(c.feels_like)),
            format!("Wind {} {}", c.wind.round() as i32, w.units.wind()),
            format!("Humidity {}%", c.humidity),
        ],
        days: w
            .days
            .iter()
            .take(5)
            .map(|d| ForecastDay {
                name: if Some(d.date) == today { "TODAY".into() } else { d.date.weekday().to_string().to_uppercase() },
                high: degrees(d.high),
                low: degrees(d.low),
                condition: Condition::from_wmo(d.code),
                precipitation: d.precipitation.filter(|p| *p >= 30).map(|p| format!("{p}%")),
            })
            .collect(),
    }
}

/// The group to show: a favorite's, else the featured game's home team's,
/// else the first followed league's first group. Returns the teams to
/// highlight too.
fn pick_standings<'a>(
    all: &'a [Standings],
    favorites: &[TeamId],
    featured: Option<&Game>,
) -> Option<(&'a Standings, &'a StandingsGroup, Vec<TeamId>)> {
    for fav in favorites {
        for s in all {
            if let Some(g) = s.group_of(fav) {
                let marked = favorites.iter().filter(|f| g.rows.iter().any(|r| &r.team == *f)).cloned().collect();
                return Some((s, g, marked));
            }
        }
    }
    if let Some(game) = featured
        && let Some(s) = all.iter().find(|s| s.league == game.league)
        && let Some(g) = s.group_of(&game.home.team.id).or_else(|| s.group_of(&game.away.team.id))
    {
        return Some((s, g, vec![game.home.team.id.clone(), game.away.team.id.clone()]));
    }
    let s = all.first()?;
    Some((s, s.groups.first()?, Vec::new()))
}

pub fn standings_view(all: &[Standings], favorites: &[TeamId], featured: Option<&Game>) -> Option<StandingsView> {
    let (s, group, marked) = pick_standings(all, favorites, featured)?;
    let league = league_label(s.league.as_str());
    let columns = standings::columns(s.sport);
    let rows: Vec<StandingsLine> = group
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| StandingsLine {
            rank: i as u32 + 1,
            team: r.abbreviation.clone(),
            cells: columns.iter().map(|(_, cell)| cell(r)).collect(),
            highlight: marked.contains(&r.team),
        })
        .collect();
    Some(StandingsView {
        title: "STANDINGS".into(),
        group: if s.groups.len() == 1 { league } else { format!("{league}  |  {}", group.name.to_uppercase()) },
        columns: columns.iter().map(|(h, _)| (*h).to_owned()).collect(),
        focus: rows.iter().position(|r| r.highlight),
        rows,
    })
}

/// Picks the most interesting game: a live game involving a favorite, else
/// the closest live game (small margin, late in the game), else the next game
/// to start, else the most recent final.
pub fn pick_game_of_the_day<'a>(games: &'a [Game], favorites: &[TeamId], now: DateTime<Utc>) -> Option<&'a Game> {
    let is_fav = |g: &Game| favorites.contains(&g.home.team.id) || favorites.contains(&g.away.team.id);
    let margin = |g: &Game| g.home.score.unwrap_or(0).abs_diff(g.away.score.unwrap_or(0));
    let live = || games.iter().filter(|g| g.status.is_live());
    if let Some(g) = live().filter(|g| is_fav(g)).min_by_key(|g| margin(g)) {
        return Some(g);
    }
    // Closer and later is better: sort by margin, then by period descending.
    if let Some(g) = live().min_by_key(|g| (margin(g), std::cmp::Reverse(g.clock.period))) {
        return Some(g);
    }
    let upcoming = games.iter().filter(|g| g.status == GameStatus::Scheduled && g.start_time >= now);
    if let Some(g) = upcoming.min_by_key(|g| (!is_fav(g), g.start_time)) {
        return Some(g);
    }
    games.iter().filter(|g| g.status == GameStatus::Final).max_by_key(|g| g.start_time)
}

fn side(c: &Competitor, label: &str, final_: bool) -> Side {
    let record = c.team.record.as_deref().map(|r| format!("  ·  {r}")).unwrap_or_default();
    Side {
        abbr: c.team.abbreviation.clone(),
        detail: format!("{label}{record}"),
        score: c.score,
        color: led_team_color(c.team.colors.primary, c.team.colors.secondary),
        lost: final_ && c.winner == Some(false),
    }
}

fn start_text(g: &Game, tz: FixedOffset, now: DateTime<Utc>) -> String {
    let local = g.start_time.with_timezone(&tz);
    let time = local.format("%-I:%M %p").to_string();
    if local.date_naive() == now.with_timezone(&tz).date_naive() {
        time
    } else {
        format!("{} {time}", local.weekday().to_string().to_uppercase())
    }
}

/// Short status for a game: "Q3 4:31", "TOP 7", "HALF", "FINAL", "4:30 PM".
pub fn status_text(g: &Game, tz: FixedOffset, now: DateTime<Utc>) -> (String, Tone) {
    match g.status {
        GameStatus::InProgress => {
            let text = match (&g.situation, &g.clock.clock) {
                // Words, not ▲/▼: widget text uses a regular font without arrows.
                (Some(Situation::Baseball { half, .. }), _) => {
                    format!("{} {}", if *half == InningHalf::Top { "TOP" } else { "BOT" }, g.clock.period)
                }
                (_, Some(clock)) if !g.clock.period_label.is_empty() => format!("{} {clock}", g.clock.period_label),
                _ => g.clock.period_label.clone(),
            };
            (text, Tone::Live)
        }
        GameStatus::Halftime => ("HALF".into(), Tone::Break),
        GameStatus::Delayed => ("DELAY".into(), Tone::Break),
        GameStatus::Final => {
            let d = g.clock.detail.to_uppercase();
            let text = d.strip_prefix("FINAL/").map_or("FINAL".to_owned(), |rest| format!("F/{rest}"));
            (text, Tone::Final)
        }
        GameStatus::Scheduled => (start_text(g, tz, now), Tone::Upcoming),
        GameStatus::Postponed => ("PPD".into(), Tone::Final),
        GameStatus::Canceled => ("CANC".into(), Tone::Final),
    }
}

fn chips(g: &Game) -> Vec<String> {
    match &g.situation {
        Some(Situation::Football { possession, down_distance_text, down, distance, .. }) => {
            let mut out = Vec::new();
            if let Some(p) = possession {
                let abbr =
                    [&g.home, &g.away].into_iter().find(|c| &c.team.id == p).map(|c| c.team.abbreviation.clone());
                if let Some(abbr) = abbr {
                    out.push(format!("{abbr} ball"));
                }
            }
            if let (Some(d), Some(dist)) = (down, distance) {
                let ord = match d {
                    1 => "1st",
                    2 => "2nd",
                    3 => "3rd",
                    _ => "4th",
                };
                out.push(format!("{ord} & {dist}"));
            }
            if let Some(spot) = down_distance_text.as_deref().and_then(|t| t.split(" at ").nth(1)) {
                out.push(spot.to_owned());
            }
            out
        }
        Some(Situation::Baseball { outs, bases, .. }) => {
            let mut out = vec![if *outs == 1 { "1 out".into() } else { format!("{outs} outs") }];
            let on: Vec<&str> =
                ["1st", "2nd", "3rd"].into_iter().zip(bases).filter(|(_, on)| **on).map(|(b, _)| b).collect();
            out.push(match on.len() {
                0 => "Bases empty".into(),
                3 => "Bases loaded".into(),
                _ => format!("On {}", on.join(" & ")),
            });
            out
        }
        Some(Situation::Hockey { power_play: Some(p) }) => [&g.home, &g.away]
            .into_iter()
            .find(|c| &c.team.id == p)
            .map(|c| vec![format!("{} power play", c.team.abbreviation)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn line_score(g: &Game) -> (Vec<String>, Vec<(String, Vec<String>)>) {
    let periods = g.away.linescore.len().max(g.home.linescore.len());
    let regulation = match g.sport {
        Sport::Football | Sport::Basketball => 4,
        Sport::Hockey => 3,
        Sport::Baseball => 9,
        Sport::Soccer => 2,
    };
    let n = periods.max(regulation);
    let header = |i: usize| match g.sport {
        Sport::Football | Sport::Basketball if i < 4 => format!("Q{}", i + 1),
        Sport::Football | Sport::Basketball => {
            if i == 4 {
                "OT".into()
            } else {
                format!("{}OT", i - 3)
            }
        }
        Sport::Hockey if i < 3 => (i + 1).to_string(),
        Sport::Hockey => "OT".into(),
        Sport::Soccer if i < 2 => format!("{}H", i + 1),
        Sport::Soccer => "ET".into(),
        Sport::Baseball => (i + 1).to_string(),
    };
    let mut columns: Vec<String> = (0..n).map(header).collect();
    columns.push(if g.sport == Sport::Baseball { "R".into() } else { "T".into() });
    let row = |c: &Competitor| {
        let mut cells: Vec<String> =
            (0..n).map(|i| c.linescore.get(i).map_or("–".to_owned(), ToString::to_string)).collect();
        cells.push(c.score.map_or("–".to_owned(), |s| s.to_string()));
        (c.team.abbreviation.clone(), cells)
    };
    (columns, vec![row(&g.away), row(&g.home)])
}

pub fn game_of_the_day(g: &Game, tz: FixedOffset, now: DateTime<Utc>) -> GameOfTheDay {
    let (status, tone) = status_text(g, tz, now);
    let final_ = g.status == GameStatus::Final;
    let (columns, rows) = line_score(g);
    GameOfTheDay {
        status: format!("{}  |  {status}", league_label(g.league.as_str())),
        live: matches!(tone, Tone::Live | Tone::Break),
        away: side(&g.away, "Away", final_),
        home: side(&g.home, "Home", final_),
        chips: chips(g),
        columns,
        rows,
    }
}

/// Scores list: live games first, then finals, then upcoming; at most `limit` rows.
pub fn scores(games: &[Game], tz: FixedOffset, now: DateTime<Utc>, limit: usize) -> Scores {
    let rank = |t: Tone| match t {
        Tone::Live | Tone::Break => 0,
        Tone::Final => 1,
        Tone::Upcoming => 2,
    };
    let mut rows: Vec<(u8, DateTime<Utc>, ScoreRow)> = games
        .iter()
        .map(|g| {
            let (status, tone) = status_text(g, tz, now);
            let team = |c: &Competitor| match c.score {
                Some(s) if g.status != GameStatus::Scheduled => format!("{} {s}", c.team.abbreviation),
                _ => c.team.abbreviation.clone(),
            };
            let row = ScoreRow {
                league: league_label(g.league.as_str()),
                away: team(&g.away),
                home: team(&g.home),
                status,
                tone,
            };
            (rank(tone), g.start_time, row)
        })
        .collect();
    rows.sort_by_key(|(r, t, _)| (*r, *t));
    Scores { title: "SCORES".into(), rows: rows.into_iter().take(limit).map(|(_, _, r)| r).collect() }
}

/// What widgets are built from.
#[derive(Clone, Copy, Debug, Default)]
pub struct WidgetData<'a> {
    pub games: &'a [Game],
    pub standings: &'a [Standings],
    pub weather: Option<&'a Weather>,
    pub favorites: &'a [TeamId],
}

/// Views for the configured widget slots. When a Game of the Day is shown,
/// the scores list leaves that game out.
pub fn build_views(
    kinds: &[WidgetKind],
    data: &WidgetData<'_>,
    tz: FixedOffset,
    now: DateTime<Utc>,
) -> Vec<WidgetView> {
    let WidgetData { games, standings, weather, favorites } = *data;
    let featured =
        kinds.contains(&WidgetKind::GameOfTheDay).then(|| pick_game_of_the_day(games, favorites, now)).flatten();
    let spotlight = featured.or_else(|| pick_game_of_the_day(games, favorites, now));
    let others: Vec<Game> = games.iter().filter(|g| Some(&g.id) != featured.map(|f| &f.id)).cloned().collect();
    kinds
        .iter()
        .map(|kind| match kind {
            WidgetKind::GameOfTheDay => {
                featured.map(|g| WidgetView::GameOfTheDay(game_of_the_day(g, tz, now))).unwrap_or_else(|| {
                    WidgetView::Empty { title: "GAME OF THE DAY".into(), message: "No games today".into() }
                })
            }
            WidgetKind::Scores => WidgetView::Scores(scores(&others, tz, now, 8)),
            WidgetKind::Standings => standings_view(standings, favorites, spotlight).map_or_else(
                || WidgetView::Empty { title: "STANDINGS".into(), message: "No standings yet".into() },
                WidgetView::Standings,
            ),
            WidgetKind::Weather => weather.map_or_else(
                || WidgetView::Empty { title: "WEATHER".into(), message: "Set a location on the admin page".into() },
                |w| WidgetView::Weather(weather_view(w)),
            ),
        })
        .collect()
}

/// The default widget area: game of the day plus a scores list.
pub fn default_views(games: &[Game], favorites: &[TeamId], tz: FixedOffset, now: DateTime<Utc>) -> Vec<WidgetView> {
    build_views(&Settings::default().widgets, &WidgetData { games, favorites, ..WidgetData::default() }, tz, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures::mock_games;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap()
    }

    fn tz() -> FixedOffset {
        FixedOffset::west_opt(4 * 3600).unwrap()
    }

    #[test]
    fn picks_the_closest_live_game_unless_a_favorite_is_playing() {
        let games = mock_games(now());
        // Live: KC-BUF (4), LAD-CHC (1), NYL-LV (3), MTL-TOR (1), ARS-LIV (0); halftime ALA-UGA (4).
        assert_eq!(pick_game_of_the_day(&games, &[], now()).unwrap().id.0, "mock:epl:1", "a draw is closest");
        let buf = games[0].home.team.id.clone();
        assert_eq!(pick_game_of_the_day(&games, &[buf], now()).unwrap().id.0, "mock:nfl:1");
    }

    #[test]
    fn falls_back_to_upcoming_then_final() {
        let mut games = mock_games(now());
        games.retain(|g| !g.status.is_live());
        assert_eq!(pick_game_of_the_day(&games, &[], now()).unwrap().id.0, "mock:epl:2", "starts soonest");
        games.retain(|g| g.status == GameStatus::Final);
        assert_eq!(pick_game_of_the_day(&games, &[], now()).unwrap().id.0, "mock:nfl:2", "most recent final");
        assert!(pick_game_of_the_day(&[], &[], now()).is_none());
    }

    #[test]
    fn football_game_of_the_day() {
        let g = mock_games(now()).into_iter().find(|g| g.id.0 == "mock:nfl:1").unwrap();
        let v = game_of_the_day(&g, tz(), now());
        assert_eq!(v.status, "NFL  |  Q3 4:32");
        assert!(v.live);
        assert_eq!((v.away.abbr.as_str(), v.away.score), ("KC", Some(17)));
        assert_eq!(v.home.detail, "Home");
        assert_eq!(v.chips, ["BUF ball", "2nd & 6", "KC 14"]);
        assert_eq!(v.columns, ["Q1", "Q2", "Q3", "Q4", "T"]);
        assert_eq!(v.rows[1], ("BUF".into(), vec!["–".into(), "–".into(), "–".into(), "–".into(), "21".into()]));
    }

    #[test]
    fn baseball_chips_and_innings() {
        let g = mock_games(now()).into_iter().find(|g| g.id.0 == "mock:mlb:1").unwrap();
        let v = game_of_the_day(&g, tz(), now());
        assert_eq!(v.status, "MLB  |  TOP 7");
        assert_eq!(v.chips, ["1 out", "On 1st & 3rd"]);
        assert_eq!(v.columns.len(), 10, "9 innings + R");
        assert_eq!(v.columns.last().unwrap(), "R");
    }

    #[test]
    fn scores_list_orders_live_final_upcoming_and_caps_rows() {
        let s = scores(&mock_games(now()), tz(), now(), 20);
        let tones: Vec<Tone> = s.rows.iter().map(|r| r.tone).collect();
        let first_final = tones.iter().position(|t| *t == Tone::Final).unwrap();
        let first_upcoming = tones.iter().position(|t| *t == Tone::Upcoming).unwrap();
        assert!(tones[..first_final].iter().all(|t| matches!(t, Tone::Live | Tone::Break)));
        assert!(first_final < first_upcoming);
        let upcoming = &s.rows[first_upcoming];
        assert_eq!((upcoming.away.as_str(), upcoming.status.as_str()), ("MUN", "4:30 PM"), "no score before kickoff");
        let fin = s.rows.iter().find(|r| r.away.starts_with("PHI")).unwrap();
        assert_eq!((fin.away.as_str(), fin.status.as_str()), ("PHI 27", "FINAL"));
        assert_eq!(scores(&mock_games(now()), tz(), now(), 3).rows.len(), 3);
    }

    #[test]
    fn default_views_do_not_repeat_the_featured_game_and_handle_empty_days() {
        let views = default_views(&mock_games(now()), &[], tz(), now());
        let WidgetView::Scores(s) = &views[1] else { panic!() };
        assert!(!s.rows.iter().any(|r| r.away.starts_with("ARS")), "EPL draw is featured, not listed");
        let empty = default_views(&[], &[], tz(), now());
        assert!(matches!(empty[0], WidgetView::Empty { .. }));
    }

    #[test]
    fn slots_follow_settings() {
        let games = mock_games(now());
        let two_lists = build_views(
            &[WidgetKind::Scores, WidgetKind::Scores],
            &WidgetData { games: &games, ..WidgetData::default() },
            tz(),
            now(),
        );
        let WidgetView::Scores(s) = &two_lists[0] else { panic!() };
        assert!(s.rows.iter().any(|r| r.away.starts_with("ARS")), "no featured game, so nothing is left out");
    }

    #[test]
    fn views_round_trip_as_json() {
        let views = default_views(&mock_games(now()), &[], tz(), now());
        let json = serde_json::to_string(&views).unwrap();
        assert_eq!(serde_json::from_str::<Vec<WidgetView>>(&json).unwrap(), views);
    }

    #[test]
    fn standings_follow_a_favorite_then_the_featured_game() {
        use crate::sports::fixtures::mock_standings;
        let (games, all) = (mock_games(now()), mock_standings(now()));
        let fav = TeamId("mock:nfl:NYJ".into());
        let v = standings_view(&all, std::slice::from_ref(&fav), None).unwrap();
        assert_eq!(v.group, "NFL  |  AFC EAST");
        assert_eq!(v.columns, ["W", "L", "T", "PCT"]);
        assert_eq!(v.rows[0].cells, ["3", "0", "0", "1.000"]);
        assert_eq!(v.rows[2].team, "NYJ");
        assert_eq!((v.focus, v.rows[2].highlight, v.rows[0].highlight), (Some(2), true, false));

        // No favorite: the featured game's division, both teams marked.
        let kc_buf = games.iter().find(|g| g.id.0 == "mock:nfl:1").unwrap();
        let v = standings_view(&all, &[], Some(kc_buf)).unwrap();
        assert_eq!(v.group, "NFL  |  AFC EAST", "home team's group");
        assert!(v.rows.iter().find(|r| r.team == "BUF").unwrap().highlight);

        // A single table is labeled by its league only.
        let epl = standings_view(&all[1..], &[TeamId("mock:epl:MUN".into())], None).unwrap();
        assert_eq!((epl.group.as_str(), epl.focus), ("EPL", Some(8)));
        assert_eq!(epl.columns, ["P", "W", "D", "L", "PTS"]);
        assert_eq!(epl.rows[0].cells, ["5", "5", "0", "0", "15"]);

        // Nothing at all: the widget says so.
        let data = WidgetData { games: &games, ..WidgetData::default() };
        let views = build_views(&[WidgetKind::Standings], &data, tz(), now());
        assert!(matches!(&views[0], WidgetView::Empty { title, .. } if title == "STANDINGS"));
    }

    #[test]
    fn weather_view_formats_for_the_card() {
        use crate::weather::mock_weather;
        let w = mock_weather(now());
        let v = weather_view(&w);
        assert_eq!((v.temperature.as_str(), v.summary.as_str()), ("72°", "Mostly clear"));
        assert_eq!(v.condition, Condition::Clear);
        assert_eq!(v.details, ["Feels 70°", "Wind 9 mph", "Humidity 55%"]);
        let names: Vec<&str> = v.days.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["TODAY", "MON", "TUE", "WED", "THU"]);
        assert_eq!(v.days[2].precipitation.as_deref(), Some("80%"));
        assert_eq!(v.days[1].precipitation, None, "10% isn't worth mentioning");
        assert_eq!(v.days[2].condition, Condition::Rain);

        let none = build_views(&[WidgetKind::Weather], &WidgetData::default(), tz(), now());
        assert!(matches!(&none[0], WidgetView::Empty { message, .. } if message.contains("location")));
        let data = WidgetData { weather: Some(&w), ..WidgetData::default() };
        assert!(matches!(build_views(&[WidgetKind::Weather], &data, tz(), now())[0], WidgetView::Weather(_)));
    }
}
