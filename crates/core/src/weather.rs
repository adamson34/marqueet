//! Weather, normalized: current conditions and a few days of forecast for
//! one place. Providers fill these in. Pure.

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::alert::{Alert, AlertLevel, Takeover};
use crate::color::Rgb;
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

/// How bad a weather alert is (CAP severity), worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Extreme,
    Severe,
    Moderate,
    Minor,
    Unknown,
}

/// An official weather alert (e.g. from the US National Weather Service).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeatherAlert {
    pub id: String,
    /// "Tornado Warning", "Flood Watch", "Wind Advisory".
    pub event: String,
    pub severity: Severity,
    /// Urgency is Immediate (act now).
    pub immediate: bool,
    /// "Jackson, MO; Johnson, KS"
    pub area: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends: Option<DateTime<Utc>>,
    /// Who issued it: "NWS Kansas City/Pleasant Hill MO".
    pub sender: String,
}

impl WeatherAlert {
    pub fn is_warning(&self) -> bool {
        self.event.ends_with("Warning")
    }

    /// Shown on the ticker while active (moderate and worse).
    pub fn on_ticker(&self) -> bool {
        self.severity <= Severity::Moderate
    }

    /// Red for warnings, orange for watches, amber for the rest.
    pub fn color(&self) -> Rgb {
        if self.is_warning() {
            Rgb::new(255, 45, 35)
        } else if self.event.ends_with("Watch") {
            Rgb::new(255, 140, 0)
        } else {
            Rgb::new(255, 196, 0)
        }
    }

    fn icon(&self) -> Option<&'static str> {
        let e = self.event.to_lowercase();
        let any = |words: &[&str]| words.iter().any(|w| e.contains(w));
        if any(&["tornado", "thunderstorm", "lightning"]) {
            Some("thunder")
        } else if any(&["flood", "rain", "hurricane", "tropical"]) {
            Some("rain")
        } else if any(&["snow", "winter", "blizzard", "ice", "freez", "frost"]) {
            Some("snow")
        } else if any(&["fog", "smoke", "dust"]) {
            Some("fog")
        } else {
            None
        }
    }

    /// "UNTIL 9:30 PM", or "UNTIL THU 1:15 AM" when it runs past today.
    pub fn until(&self, tz: FixedOffset, now: DateTime<Utc>) -> Option<String> {
        let end = self.ends?.with_timezone(&tz);
        let time = end.format("%-I:%M %p").to_string();
        Some(if end.date_naive() == now.with_timezone(&tz).date_naive() {
            format!("UNTIL {time}")
        } else {
            format!("UNTIL {} {time}", end.weekday().to_string().to_uppercase())
        })
    }

    /// "JACKSON, MO", or "JACKSON, MO + 2 MORE".
    pub fn short_area(&self) -> String {
        let places: Vec<&str> = self.area.split(';').map(str::trim).filter(|p| !p.is_empty()).collect();
        match places.as_slice() {
            [] => String::new(),
            [one] => one.to_uppercase(),
            [first, rest @ ..] => format!("{} + {} MORE", first.to_uppercase(), rest.len()),
        }
    }

    pub fn segment_id(&self) -> String {
        format!("weather-alert:{}", self.id)
    }

    /// The ticker segment shown while the alert is active.
    pub fn ticker_segment(&self, tz: FixedOffset, now: DateTime<Utc>) -> TickerSegment {
        let mut parts = Vec::new();
        if let Some(icon) = self.icon() {
            parts.push(Part::icon(icon));
            parts.push(Part::gap(3));
        }
        let bottom = [self.until(tz, now), Some(self.short_area()).filter(|a| !a.is_empty())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("  ");
        parts.push(Part::stack(
            vec![Span::new(self.event.to_uppercase(), Tint::Color(self.color()))],
            vec![Span::dim(bottom)],
            Align::Left,
        ));
        TickerSegment { id: self.segment_id(), parts }
    }

    /// What to do when the alert first appears: warnings that are severe or
    /// worse take over the screen, other severe alerts flash, lesser ones
    /// just sit on the ticker.
    pub fn to_alert(&self, tz: FixedOffset, now: DateTime<Utc>) -> Option<Alert> {
        if self.severity > Severity::Severe {
            return None;
        }
        let takeover = self.is_warning();
        let until = self.until(tz, now);
        let play = [Some(self.short_area()).filter(|a| !a.is_empty()), until].into_iter().flatten().collect::<Vec<_>>();
        Some(Alert {
            id: format!("nws:{}", self.id),
            level: if takeover { AlertLevel::Takeover } else { AlertLevel::Flash },
            source: "weather".into(),
            segment_id: Some(self.segment_id()),
            title: self.event.to_uppercase(),
            detail: Some(play.join(", ")),
            colors: Some((self.color(), self.color())),
            takeover: takeover.then(|| Takeover {
                kicker: "NATIONAL WEATHER SERVICE".into(),
                headline: self.event.to_uppercase(),
                play: (!play.is_empty()).then(|| play.join("  ")),
                score: None,
                note: None,
            }),
            created_at: now,
        })
    }
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

    fn alert(event: &str, severity: Severity) -> WeatherAlert {
        WeatherAlert {
            id: "urn:x".into(),
            event: event.into(),
            severity,
            immediate: true,
            area: "Jackson, MO; Johnson, KS; Wyandotte, KS".into(),
            ends: Some(chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 9, 24, 1, 30, 0).unwrap()),
            sender: "NWS Kansas City".into(),
        }
    }

    #[test]
    fn weather_alerts_on_the_ticker_and_screen() {
        use crate::sports::ticker::segment_text;
        let tz = FixedOffset::west_opt(5 * 3600).unwrap();
        let now = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 9, 24, 0, 45, 0).unwrap();
        let tornado = alert("Tornado Warning", Severity::Extreme);
        assert_eq!(
            segment_text(&tornado.ticker_segment(tz, now)),
            "[thunder] TORNADO WARNING/UNTIL 8:30 PM  JACKSON, MO + 2 MORE"
        );
        let a = tornado.to_alert(tz, now).unwrap();
        assert_eq!(a.level, AlertLevel::Takeover);
        assert_eq!(a.segment_id.as_deref(), Some("weather-alert:urn:x"));
        let t = a.takeover.unwrap();
        assert_eq!((t.kicker.as_str(), t.headline.as_str()), ("NATIONAL WEATHER SERVICE", "TORNADO WARNING"));
        assert_eq!(t.play.as_deref(), Some("JACKSON, MO + 2 MORE  UNTIL 8:30 PM"));

        let watch = alert("Severe Thunderstorm Watch", Severity::Severe);
        assert_eq!(watch.to_alert(tz, now).unwrap().level, AlertLevel::Flash, "watches flash");
        assert_eq!(watch.color(), Rgb::new(255, 140, 0));
        let advisory = alert("Wind Advisory", Severity::Moderate);
        assert!(advisory.to_alert(tz, now).is_none() && advisory.on_ticker(), "advisories just sit on the ticker");
        assert!(!alert("Special Weather Statement", Severity::Minor).on_ticker());

        let next_day = WeatherAlert { ends: Some(now + chrono::Duration::days(1)), ..tornado.clone() };
        assert_eq!(next_day.until(tz, now).as_deref(), Some("UNTIL THU 7:45 PM"));
        assert_eq!(WeatherAlert { area: "Clay, WV".into(), ..tornado }.short_area(), "CLAY, WV");
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
