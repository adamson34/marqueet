//! The configured time zone (an IANA name, resolved from the system's
//! zoneinfo) or the device's own.

use chrono::{DateTime, FixedOffset, Local, Utc};
use jiff::tz::TimeZone;
use marqueet_core::settings::Settings;

/// Checks that `name` is a time zone this system knows.
pub fn validate(name: &str) -> Result<(), String> {
    TimeZone::get(name).map(|_| ()).map_err(|_| format!("unknown time zone {name:?}"))
}

/// Every time zone name the system knows, sorted.
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = jiff::tz::db().available().map(|n| n.as_str().to_owned()).collect();
    names.sort();
    names
}

/// UTC offset of the configured zone at `now`, or `None` for the device's.
pub fn configured_offset(settings: &Settings, now: DateTime<Utc>) -> Option<FixedOffset> {
    let zone = TimeZone::get(settings.time_zone.as_deref()?).ok()?;
    let ts = jiff::Timestamp::from_second(now.timestamp()).ok()?;
    FixedOffset::east_opt(zone.to_offset(ts).seconds())
}

/// UTC offset to use at `now`: the configured zone's, else the device's.
pub fn offset(settings: &Settings, now: DateTime<Utc>) -> FixedOffset {
    configured_offset(settings, now).unwrap_or_else(|| *now.with_timezone(&Local).offset())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    fn with(zone: &str) -> Settings {
        Settings { time_zone: Some(zone.into()), ..Settings::default() }
    }

    #[test]
    fn follows_daylight_saving() {
        let summer = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
        let winter = Utc.with_ymd_and_hms(2026, 12, 1, 12, 0, 0).unwrap();
        let chicago = with("America/Chicago");
        assert_eq!(offset(&chicago, summer).local_minus_utc(), -5 * 3600);
        assert_eq!(offset(&chicago, winter).local_minus_utc(), -6 * 3600);
        assert_eq!(offset(&with("Asia/Kolkata"), winter).local_minus_utc(), 5 * 3600 + 1800);
    }

    #[test]
    fn unknown_or_unset_falls_back_to_the_device() {
        let now = Utc::now();
        assert_eq!(configured_offset(&Settings::default(), now), None);
        assert_eq!(configured_offset(&with("Mars/Olympus_Mons"), now), None);
        assert_eq!(offset(&Settings::default(), now), *now.with_timezone(&Local).offset());
        assert!(validate("Mars/Olympus_Mons").unwrap_err().contains("unknown time zone"));
        assert!(validate("Europe/London").is_ok());
        assert!(names().iter().any(|n| n == "America/New_York"));
    }
}
