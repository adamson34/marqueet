//! Turns the admin form (`application/x-www-form-urlencoded` pairs) into
//! settings. Pure, so every field is testable without HTTP.

use chrono::NaiveTime;
use marqueet_core::Rgb;
use marqueet_core::config::{ScrollMode, WidgetLayout};
use marqueet_core::settings::{QuietHours, Settings, TakeoverPolicy, WidgetKind};
use marqueet_core::sports::{LeagueId, TeamId};
use marqueet_core::weather::{Place, Units};

fn field<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn all<'a>(pairs: &'a [(String, String)], key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    pairs.iter().filter(move |(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn number<T: std::str::FromStr>(pairs: &[(String, String)], key: &str, current: T) -> Result<T, String> {
    match field(pairs, key).map(str::trim) {
        None | Some("") => Ok(current),
        Some(v) => v.parse().map_err(|_| format!("{key}: {v:?} is not a number")),
    }
}

fn widget(v: &str) -> Result<WidgetKind, String> {
    match v {
        "game_of_the_day" => Ok(WidgetKind::GameOfTheDay),
        "scores" => Ok(WidgetKind::Scores),
        "standings" => Ok(WidgetKind::Standings),
        "weather" => Ok(WidgetKind::Weather),
        "fantasy" => Ok(WidgetKind::Fantasy),
        other => Err(format!("unknown widget {other:?}")),
    }
}

fn time(v: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(v.trim(), "%H:%M").map_err(|_| format!("{v:?} is not a time (HH:MM)"))
}

/// What to do with the weather location field.
#[derive(Clone, Debug, PartialEq)]
pub enum LocationChange {
    Keep,
    Clear,
    Set(Place),
    /// A place name to look up.
    Lookup(String),
}

/// Reads the `location` field: blank clears it, the current place's name
/// keeps it, "lat, lon" sets it directly, anything else is looked up.
pub fn location(pairs: &[(String, String)], current: Option<&Place>) -> LocationChange {
    let Some(v) = field(pairs, "location").map(str::trim) else { return LocationChange::Keep };
    if v.is_empty() {
        LocationChange::Clear
    } else if current.is_some_and(|p| p.name == v) {
        LocationChange::Keep
    } else if let Some(p) = Place::from_coordinates(v) {
        LocationChange::Set(p)
    } else {
        LocationChange::Lookup(v.to_owned())
    }
}

/// Settings after applying the submitted form to `current`. Leagues are the
/// checked `league` values ordered by their `order_<id>` fields; unknown
/// fields are ignored. The result is sanitized.
pub fn apply(current: &Settings, supported: &[LeagueId], pairs: &[(String, String)]) -> Result<Settings, String> {
    let mut s = current.clone();

    let mut leagues: Vec<(i64, usize, LeagueId)> = Vec::new();
    for id in all(pairs, "league") {
        let league = LeagueId::new(id);
        let Some(pos) = supported.iter().position(|l| *l == league) else {
            return Err(format!("unknown league {id:?}"));
        };
        let order = number(pairs, &format!("order_{id}"), i64::MAX)?;
        leagues.push((order, pos, league));
    }
    if leagues.is_empty() {
        return Err("pick at least one league".into());
    }
    leagues.sort();
    s.leagues = leagues.into_iter().map(|(_, _, l)| l).collect();

    s.favorites = all(pairs, "favorite").filter(|v| !v.is_empty()).map(|v| TeamId(v.to_owned())).collect();

    if let Some(v) = field(pairs, "takeovers") {
        s.takeovers = match v {
            "all" => TakeoverPolicy::All,
            "favorites" => TakeoverPolicy::Favorites,
            "off" => TakeoverPolicy::Off,
            other => return Err(format!("unknown takeover setting {other:?}")),
        };
    }

    if let Some(v) = field(pairs, "widget_layout") {
        s.display.widget_layout = WidgetLayout::from_id(v).ok_or_else(|| format!("unknown layout {v:?}"))?;
    }
    // One widget per slot of the layout; the form has a select for every
    // possible slot and extra ones are ignored.
    let slots: Vec<Option<&str>> =
        (0..s.display.widget_layout.slots()).map(|i| field(pairs, &format!("widget_{i}"))).collect();
    if slots.iter().all(Option::is_some) {
        s.widgets = slots.into_iter().flatten().map(widget).collect::<Result<_, _>>()?;
    }

    let d = &mut s.display;
    if let Some(v) = field(pairs, "led_color") {
        d.led_color = v.parse::<Rgb>().map_err(|e| e.to_string())?;
    }
    if let Some(v) = field(pairs, "scroll_mode") {
        d.scroll_mode = match v {
            "stepped" => ScrollMode::Stepped,
            "smooth" => ScrollMode::Smooth,
            other => return Err(format!("unknown scroll mode {other:?}")),
        };
    }
    d.ticker_speed = number(pairs, "ticker_speed", d.ticker_speed)?;
    d.crawl_speed = number(pairs, "crawl_speed", d.crawl_speed)?;
    d.ticker_rows = number(pairs, "ticker_rows", d.ticker_rows)?;
    d.glow = number(pairs, "glow", d.glow)?;
    d.flicker = number(pairs, "flicker", d.flicker)?;

    s.weather.ticker = field(pairs, "weather_ticker") == Some("on");
    s.weather.alerts = field(pairs, "weather_alerts") == Some("on");
    if let Some(v) = field(pairs, "units") {
        s.weather.units = match v {
            "fahrenheit" => Units::Fahrenheit,
            "celsius" => Units::Celsius,
            other => return Err(format!("unknown units {other:?}")),
        };
    }

    if let Some(v) = field(pairs, "time_zone") {
        s.time_zone = Some(v.to_owned());
    }

    s.quiet_hours = match field(pairs, "quiet_enabled") {
        Some("on") => {
            let from = time(field(pairs, "quiet_from").unwrap_or("23:00"))?;
            let to = time(field(pairs, "quiet_to").unwrap_or("07:00"))?;
            Some(QuietHours { from, to })
        }
        _ => None,
    };
    Ok(s.sanitized())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    fn supported() -> Vec<LeagueId> {
        ["nfl", "mlb", "nhl", "epl"].map(LeagueId::new).to_vec()
    }

    #[test]
    fn full_form() {
        let form = pairs(&[
            ("league", "nfl"),
            ("order_nfl", "2"),
            ("league", "epl"),
            ("order_epl", "1"),
            ("order_mlb", "0"), // not checked: ignored
            ("favorite", "espn:nfl:2"),
            ("favorite", "espn:epl:364"),
            ("takeovers", "favorites"),
            ("widget_0", "scores"),
            ("widget_1", "game_of_the_day"),
            ("led_color", "#33ccff"),
            ("ticker_speed", "30"),
            ("ticker_rows", "21"),
            ("glow", "0.4"),
            ("scroll_mode", "smooth"),
            ("quiet_enabled", "on"),
            ("quiet_from", "23:30"),
            ("quiet_to", "06:45"),
            ("time_zone", " America/Denver "),
        ]);
        let s = apply(&Settings::default(), &supported(), &form).unwrap();
        assert_eq!(s.leagues, vec![LeagueId::new("epl"), LeagueId::new("nfl")], "ordered by order_<id>");
        assert_eq!(s.favorites.len(), 2);
        assert_eq!(s.takeovers, TakeoverPolicy::Favorites);
        assert_eq!(s.widgets, vec![WidgetKind::Scores, WidgetKind::GameOfTheDay]);
        assert_eq!(s.display.led_color, Rgb::new(0x33, 0xcc, 0xff));
        assert_eq!((s.display.ticker_speed, s.display.ticker_rows, s.display.glow), (30.0, 21, 0.4));
        assert_eq!(s.display.scroll_mode, ScrollMode::Smooth);
        assert_eq!(s.time_zone.as_deref(), Some("America/Denver"));
        let q = s.quiet_hours.unwrap();
        assert_eq!((q.from.to_string(), q.to.to_string()), ("23:30:00".into(), "06:45:00".into()));
    }

    #[test]
    fn layouts_take_one_widget_per_slot() {
        let form = pairs(&[
            ("league", "nfl"),
            ("widget_layout", "three"),
            ("widget_0", "weather"),
            ("widget_1", "scores"),
            ("widget_2", "standings"),
        ]);
        let s = apply(&Settings::default(), &supported(), &form).unwrap();
        assert_eq!(s.display.widget_layout, WidgetLayout::Three);
        assert_eq!(s.widgets, vec![WidgetKind::Weather, WidgetKind::Scores, WidgetKind::Standings]);
        let single =
            pairs(&[("league", "nfl"), ("widget_layout", "single"), ("widget_0", "weather"), ("widget_1", "scores")]);
        assert_eq!(
            apply(&s, &supported(), &single).unwrap().widgets,
            vec![WidgetKind::Weather],
            "extra selects ignored"
        );
        let bad = pairs(&[("league", "nfl"), ("widget_layout", "hexagon")]);
        assert!(apply(&s, &supported(), &bad).unwrap_err().contains("layout"));
    }

    #[test]
    fn location_field() {
        let kc = Place { name: "Kansas City, Missouri".into(), latitude: 39.1, longitude: -94.58 };
        let loc = |v: &str| location(&pairs(&[("location", v)]), Some(&kc));
        assert_eq!(location(&[], Some(&kc)), LocationChange::Keep, "field absent");
        assert_eq!(loc(" Kansas City, Missouri "), LocationChange::Keep, "unchanged");
        assert_eq!(loc(""), LocationChange::Clear);
        assert_eq!(loc("Oslo"), LocationChange::Lookup("Oslo".into()));
        assert!(matches!(loc("59.91, 10.75"), LocationChange::Set(p) if p.latitude == 59.91));
        let s = apply(&Settings::default(), &supported(), &pairs(&[("league", "nfl"), ("units", "celsius")])).unwrap();
        assert_eq!(s.weather.units, Units::Celsius);
        assert!(!s.weather.ticker, "unticked");
        let s = apply(&s, &supported(), &pairs(&[("league", "nfl"), ("weather_ticker", "on")])).unwrap();
        assert!(s.weather.ticker);
    }

    #[test]
    fn unchecked_extras_keep_current_values() {
        let current = Settings { quiet_hours: None, ..Settings::default() };
        let s = apply(&current, &supported(), &pairs(&[("league", "mlb")])).unwrap();
        assert_eq!(s.leagues, vec![LeagueId::new("mlb")]);
        assert_eq!(s.display, current.display);
        assert!(s.favorites.is_empty(), "an unticked favorite is removed");
    }

    #[test]
    fn bad_input_is_explained() {
        let bad = |form: &[(&str, &str)]| apply(&Settings::default(), &supported(), &pairs(form)).unwrap_err();
        assert!(bad(&[]).contains("at least one league"));
        assert!(bad(&[("league", "curling")]).contains("unknown league"));
        assert!(bad(&[("league", "nfl"), ("ticker_speed", "fast")]).contains("not a number"));
        assert!(bad(&[("league", "nfl"), ("led_color", "orange-ish")]).contains("invalid color"));
        assert!(bad(&[("league", "nfl"), ("quiet_enabled", "on"), ("quiet_from", "late")]).contains("not a time"));
        assert!(bad(&[("league", "nfl"), ("takeovers", "sometimes")]).contains("takeover"));
    }

    #[test]
    fn values_are_clamped_by_sanitize() {
        let s =
            apply(&Settings::default(), &supported(), &pairs(&[("league", "nfl"), ("ticker_rows", "500")])).unwrap();
        assert_eq!(s.display.ticker_rows, 48);
    }
}
