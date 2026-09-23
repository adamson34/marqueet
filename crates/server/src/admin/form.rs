//! Turns the admin form (`application/x-www-form-urlencoded` pairs) into
//! settings. Pure, so every field is testable without HTTP.

use chrono::NaiveTime;
use marqueet_core::Rgb;
use marqueet_core::config::ScrollMode;
use marqueet_core::settings::{QuietHours, Settings, TakeoverPolicy, WidgetKind};
use marqueet_core::sports::{LeagueId, TeamId};

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
        other => Err(format!("unknown widget {other:?}")),
    }
}

fn time(v: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(v.trim(), "%H:%M").map_err(|_| format!("{v:?} is not a time (HH:MM)"))
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

    let slots = [field(pairs, "widget_0"), field(pairs, "widget_1")];
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
