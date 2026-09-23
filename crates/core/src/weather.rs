//! Weather, normalized: current conditions and a few days of forecast for
//! one place. Providers fill these in. Pure.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::ticker::{Align, Part, Span, TickerSegment, Tint};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    /// °F and mph.
    #[default]
    Fahrenheit,
    /// °C and km/h.
    Celsius,
}

impl Units {
    pub fn wind(self) -> &'static str {
        match self {
            Units::Fahrenheit => "mph",
            Units::Celsius => "km/h",
        }
    }
}

/// Where to get the weather for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// "Kansas City, Missouri"
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// Parses "39.1, -94.58" into a place named after its coordinates.
    pub fn from_coordinates(s: &str) -> Option<Place> {
        let (lat, lon) = s.split_once(',')?;
        let (latitude, longitude): (f64, f64) = (lat.trim().parse().ok()?, lon.trim().parse().ok()?);
        ((-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude)).then(|| Place {
            name: format!("{latitude:.2}, {longitude:.2}"),
            latitude,
            longitude,
        })
    }
}

/// Broad sky condition, from a WMO weather code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Snow,
    Thunder,
}

impl Condition {
    /// WMO 4677 codes as used by Open-Meteo.
    pub fn from_wmo(code: u16) -> Condition {
        match code {
            0 | 1 => Condition::Clear,
            2 => Condition::PartlyCloudy,
            3 => Condition::Cloudy,
            45 | 48 => Condition::Fog,
            51..=57 => Condition::Drizzle,
            61..=67 | 80..=82 => Condition::Rain,
            71..=77 | 85 | 86 => Condition::Snow,
            95..=99 => Condition::Thunder,
            _ => Condition::Cloudy,
        }
    }
}

/// Short description of a WMO code: "Mostly clear", "Heavy rain".
pub fn describe(code: u16) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Cloudy",
        45 | 48 => "Fog",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 | 81 => "Showers",
        82 => "Heavy showers",
        85 | 86 => "Snow showers",
        95 => "Thunderstorms",
        96 | 99 => "Storms with hail",
        _ => "Cloudy",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Current {
    pub temperature: f32,
    pub feels_like: f32,
    /// Percent.
    pub humidity: u8,
    pub wind: f32,
    /// WMO weather code.
    pub code: u16,
    pub is_day: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// Local date at the place.
    pub date: NaiveDate,
    pub high: f32,
    pub low: f32,
    pub code: u16,
    /// Highest chance of precipitation that day, percent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precipitation: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub place: Place,
    pub units: Units,
    pub current: Current,
    /// Today first.
    pub days: Vec<Day>,
    pub fetched_at: DateTime<Utc>,
}

/// LED icon (see `assets/icons.txt`) for a condition.
pub fn icon_name(condition: Condition, day: bool) -> &'static str {
    match (condition, day) {
        (Condition::Clear, true) => "sun",
        (Condition::Clear, false) => "moon",
        (Condition::PartlyCloudy, true) => "partly_cloudy",
        (Condition::PartlyCloudy, false) => "partly_cloudy_night",
        (Condition::Cloudy, _) => "cloud",
        (Condition::Fog, _) => "fog",
        (Condition::Drizzle | Condition::Rain, _) => "rain",
        (Condition::Snow, _) => "snow",
        (Condition::Thunder, _) => "thunder",
    }
}

/// Chance of precipitation that earns a heads-up in the ticker.
const HEADS_UP_PERCENT: u8 = 50;

/// The first wet day in the next few, as ("RAIN FRI", "80% CHANCE"). Today
/// is skipped when it's already raining or snowing (the icon says so).
fn heads_up(w: &Weather) -> Option<(String, String)> {
    let now = Condition::from_wmo(w.current.code);
    let wet_now = matches!(now, Condition::Drizzle | Condition::Rain | Condition::Snow | Condition::Thunder);
    w.days.iter().take(4).enumerate().find_map(|(i, d)| {
        let chance = d.precipitation.filter(|p| *p >= HEADS_UP_PERCENT)?;
        let kind = match Condition::from_wmo(d.code) {
            Condition::Snow => "SNOW",
            Condition::Thunder => "STORMS",
            Condition::Drizzle | Condition::Rain => "RAIN",
            _ => return None,
        };
        if i == 0 && wet_now {
            return None;
        }
        let when = if i == 0 { "TODAY".to_owned() } else { d.date.weekday().to_string().to_uppercase() };
        Some((format!("{kind} {when}"), format!("{chance}% CHANCE")))
    })
}

/// The weather's ticker segment: icon, temperature, place and today's
/// high/low, plus a heads-up when rain or snow is coming.
pub fn ticker_segment(w: &Weather) -> TickerSegment {
    let c = &w.current;
    let deg = |t: f32| format!("{}°", t.round() as i32);
    let city = w.place.name.split(',').next().unwrap_or(&w.place.name).trim().to_uppercase();
    let today = w.days.first().map(|d| format!("H{} L{}", deg(d.high), deg(d.low))).unwrap_or_default();
    let mut parts = vec![
        Part::icon(icon_name(Condition::from_wmo(c.code), c.is_day)),
        Part::gap(3),
        Part::text(vec![Span::primary(deg(c.temperature))]),
        Part::gap(3),
        Part::stack(vec![Span::primary(city)], vec![Span::dim(today)], Align::Left),
    ];
    if let Some((what, chance)) = heads_up(w) {
        parts.push(Part::gap(5));
        parts.push(Part::stack(vec![Span::new(what, Tint::Accent)], vec![Span::dim(chance)], Align::Left));
    }
    TickerSegment { id: "weather".into(), parts }
}

/// Demo weather (mock display and tests): a clear evening, rain later in
/// the week.
pub fn mock_weather(now: DateTime<Utc>) -> Weather {
    let today = now.date_naive();
    let day = |offset: i64, high: f32, low: f32, code: u16, precipitation: u8| Day {
        date: today + chrono::Duration::days(offset),
        high,
        low,
        code,
        precipitation: Some(precipitation),
    };
    Weather {
        place: Place { name: "Kansas City, Missouri".into(), latitude: 39.1, longitude: -94.58 },
        units: Units::Fahrenheit,
        current: Current { temperature: 71.8, feels_like: 69.5, humidity: 55, wind: 8.9, code: 1, is_day: true },
        days: vec![
            day(0, 74.1, 60.3, 2, 5),
            day(1, 77.7, 57.0, 3, 10),
            day(2, 66.0, 55.2, 63, 80),
            day(3, 68.4, 52.1, 80, 45),
            day(4, 79.2, 58.8, 0, 0),
        ],
        fetched_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmo_codes_map_to_conditions() {
        assert_eq!(Condition::from_wmo(0), Condition::Clear);
        assert_eq!(Condition::from_wmo(2), Condition::PartlyCloudy);
        assert_eq!(Condition::from_wmo(63), Condition::Rain);
        assert_eq!(Condition::from_wmo(81), Condition::Rain);
        assert_eq!(Condition::from_wmo(75), Condition::Snow);
        assert_eq!(Condition::from_wmo(96), Condition::Thunder);
        assert_eq!(Condition::from_wmo(1234), Condition::Cloudy, "unknown codes are just cloudy");
        assert_eq!(describe(65), "Heavy rain");
    }

    #[test]
    fn ticker_segment_reads_like_a_sign() {
        use crate::sports::ticker::segment_text;
        let now = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 9, 23, 20, 0, 0).unwrap();
        let mut w = mock_weather(now);
        assert_eq!(segment_text(&ticker_segment(&w)), "[sun] 72° KANSAS CITY/H74° L60° RAIN FRI/80% CHANCE");
        w.current.is_day = false;
        w.days[2].precipitation = Some(20);
        w.days[3].precipitation = Some(20);
        assert_eq!(segment_text(&ticker_segment(&w)), "[moon] 72° KANSAS CITY/H74° L60°", "nothing coming");
        w.current.code = 63;
        w.days[0] = Day { code: 63, precipitation: Some(90), ..w.days[0].clone() };
        let seg = segment_text(&ticker_segment(&w));
        assert!(seg.starts_with("[rain]") && !seg.contains("RAIN TODAY"), "{seg}");
        w.days[0].code = 75;
        w.current.code = 0;
        assert!(segment_text(&ticker_segment(&w)).ends_with("SNOW TODAY/90% CHANCE"));
    }

    #[test]
    fn coordinates_parse_and_range_check() {
        let p = Place::from_coordinates(" 39.0997, -94.5786 ").unwrap();
        assert_eq!((p.latitude, p.longitude), (39.0997, -94.5786));
        assert_eq!(p.name, "39.10, -94.58");
        assert!(Place::from_coordinates("Kansas City").is_none());
        assert!(Place::from_coordinates("91, 0").is_none());
        assert!(Place::from_coordinates("0, 181").is_none());
    }
}
