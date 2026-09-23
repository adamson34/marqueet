//! Parsing tests against saved Open-Meteo responses (see fixtures/README.md).
#![allow(clippy::unwrap_used)]

use chrono::{NaiveDate, Utc};
use marqueet_core::weather::{Place, Units};
use marqueet_provider_openmeteo::{parse_forecast, parse_search};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn kc() -> Place {
    Place { name: "Kansas City, Missouri".into(), latitude: 39.1, longitude: -94.58 }
}

#[test]
fn forecast_in_fahrenheit() {
    let w = parse_forecast(&fixture("forecast_kc"), &kc(), Units::Fahrenheit, Utc::now()).unwrap();
    assert_eq!(w.current.temperature, 71.8);
    assert_eq!(w.current.feels_like, 69.5);
    assert_eq!((w.current.humidity, w.current.code, w.current.is_day), (55, 0, true));
    assert_eq!(w.days.len(), 5);
    assert_eq!(w.days[0].date, NaiveDate::from_ymd_opt(2026, 9, 23).unwrap());
    assert_eq!((w.days[0].high, w.days[0].low), (74.1, 60.3));
    assert_eq!(w.days[2].code, 63);
    assert_eq!(w.place, kc());
}

#[test]
fn forecast_in_celsius_at_night() {
    let oslo = Place { name: "Oslo, Norway".into(), latitude: 59.91, longitude: 10.75 };
    let w = parse_forecast(&fixture("forecast_oslo_celsius"), &oslo, Units::Celsius, Utc::now()).unwrap();
    assert_eq!(w.current.temperature, 14.1);
    assert!(!w.current.is_day);
    assert_eq!(w.units, Units::Celsius);
}

#[test]
fn search_names_places_readably() {
    let places = parse_search(&fixture("search_kansas_city")).unwrap();
    let names: Vec<&str> = places.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        ["Kansas City, Missouri", "Kansas City, Kansas", "Kansas City, South Caribbean Coast, Nicaragua"]
    );
    assert_eq!((places[0].latitude, places[0].longitude), (39.09973, -94.57857));
    assert!(parse_search(&fixture("search_nothing")).unwrap().is_empty(), "no results");
}

#[test]
fn junk_and_gaps_are_errors_or_skipped_not_panics() {
    assert!(parse_forecast("nope", &kc(), Units::Celsius, Utc::now()).is_err());
    assert!(parse_forecast("{}", &kc(), Units::Celsius, Utc::now()).is_err(), "no current conditions");
    let gappy = r#"{"current":{"temperature_2m":5},"daily":{"time":["2026-01-01","2026-01-02","bad"],
        "temperature_2m_max":[7,null,3],"temperature_2m_min":[1,2,0],"weather_code":[71,3,3]}}"#;
    let w = parse_forecast(gappy, &kc(), Units::Celsius, Utc::now()).unwrap();
    assert_eq!(w.days.len(), 1, "days missing a high, or with a bad date, are dropped");
    assert_eq!((w.current.feels_like, w.current.code), (5.0, 3));
    assert!(parse_search("not json").is_err());
}
