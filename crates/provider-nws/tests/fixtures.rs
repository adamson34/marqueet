//! Parsing tests against saved NWS responses (see fixtures/README.md).
#![allow(clippy::unwrap_used)]

use chrono::{TimeZone, Utc};
use marqueet_core::provider::ProviderError;
use marqueet_core::weather::Severity;
use marqueet_provider_nws::{parse_alerts, parse_problem};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn real_severe_alerts() {
    let now = Utc.with_ymd_and_hms(2026, 9, 24, 0, 30, 0).unwrap();
    let alerts = parse_alerts(&fixture("active_severe"), now).unwrap();
    assert_eq!(alerts.len(), 4);
    assert!(alerts.iter().all(|a| a.severity == Severity::Severe && a.is_warning()));
    let flash = alerts.iter().find(|a| a.event == "Flash Flood Warning").unwrap();
    assert!(flash.immediate);
    assert_eq!(flash.ends.unwrap(), Utc.with_ymd_and_hms(2026, 9, 24, 2, 45, 0).unwrap());
    assert_eq!(alerts[0].event, "Flash Flood Warning", "immediate ones first");
    let later = Utc.with_ymd_and_hms(2026, 9, 24, 4, 0, 0).unwrap();
    assert_eq!(parse_alerts(&fixture("active_severe"), later).unwrap().len(), 1, "ended alerts are dropped");
}

#[test]
fn tests_and_cancellations_are_dropped_worst_first() {
    let now = Utc.with_ymd_and_hms(2026, 9, 24, 0, 45, 0).unwrap();
    let alerts = parse_alerts(&fixture("synthetic_kc"), now).unwrap();
    let events: Vec<&str> = alerts.iter().map(|a| a.event.as_str()).collect();
    assert_eq!(events, ["Tornado Warning", "Severe Thunderstorm Watch", "Wind Advisory", "Special Weather Statement"]);
    assert_eq!(alerts[0].area, "Jackson, MO; Johnson, KS; Wyandotte, KS");
}

#[test]
fn quiet_days_and_other_countries() {
    assert!(parse_alerts(&fixture("point_none"), Utc::now()).unwrap().is_empty());
    assert!(matches!(parse_problem(400, &fixture("point_out_of_bounds")), ProviderError::Unsupported(_)));
    assert_eq!(parse_problem(503, "oops"), ProviderError::Status(503));
    assert!(parse_alerts("nope", Utc::now()).is_err());
}
