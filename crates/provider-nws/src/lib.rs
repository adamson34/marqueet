//! Severe weather alerts from the US National Weather Service
//! (`api.weather.gov`): free, no key, public domain. Covers the US and its
//! territories; elsewhere every request answers "unsupported".
//!
//! Only real, current alerts are kept: test and exercise messages and
//! cancellations are dropped. Parsing is pure and tested against saved
//! responses.

use std::time::Duration;

use chrono::{DateTime, Utc};
use marqueet_core::provider::{BoxFuture, ProviderError, WeatherAlertsProvider};
use marqueet_core::weather::{Place, Severity, WeatherAlert};
use serde::Deserialize;

pub const BASE_URL: &str = "https://api.weather.gov";
/// NWS asks for a User-Agent that identifies the app and a way to reach it.
pub const USER_AGENT: &str =
    concat!("marqueet/", env!("CARGO_PKG_VERSION"), " (+https://github.com/adamson34/marqueet)");

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Collection {
    features: Vec<Feature>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Feature {
    properties: Properties,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Properties {
    id: String,
    event: String,
    severity: String,
    urgency: String,
    status: String,
    message_type: String,
    area_desc: String,
    ends: Option<String>,
    expires: Option<String>,
    sender_name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Problem {
    detail: String,
}

fn severity(s: &str) -> Severity {
    match s {
        "Extreme" => Severity::Extreme,
        "Severe" => Severity::Severe,
        "Moderate" => Severity::Moderate,
        "Minor" => Severity::Minor,
        _ => Severity::Unknown,
    }
}

fn time(s: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s?).ok().map(|t| t.with_timezone(&Utc))
}

/// Parses an `/alerts/active` response, keeping actual alerts and updates
/// that haven't ended by `now`, worst first.
pub fn parse_alerts(body: &str, now: DateTime<Utc>) -> Result<Vec<WeatherAlert>, ProviderError> {
    let parsed: Collection = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let mut out: Vec<WeatherAlert> = parsed
        .features
        .into_iter()
        .map(|f| f.properties)
        .filter(|p| p.status == "Actual" && matches!(p.message_type.as_str(), "Alert" | "Update"))
        .filter(|p| !p.id.is_empty() && !p.event.is_empty())
        .map(|p| WeatherAlert {
            ends: time(p.ends.as_deref()).or_else(|| time(p.expires.as_deref())),
            id: p.id,
            event: p.event,
            severity: severity(&p.severity),
            immediate: p.urgency == "Immediate",
            area: p.area_desc,
            sender: p.sender_name,
        })
        .filter(|a| a.ends.is_none_or(|e| e > now))
        .collect();
    out.sort_by_key(|a| (a.severity, !a.immediate));
    Ok(out)
}

/// Turns an error response into a [`ProviderError`]: points outside NWS
/// coverage are `Unsupported`.
pub fn parse_problem(status: u16, body: &str) -> ProviderError {
    let problem: Problem = serde_json::from_str(body).unwrap_or_default();
    if status == 400 && problem.detail.contains("out of bounds") {
        ProviderError::Unsupported("weather alerts outside the US".into())
    } else {
        ProviderError::Status(status)
    }
}

#[derive(Debug, Clone)]
pub struct Nws {
    client: reqwest::Client,
    base_url: String,
}

impl Nws {
    pub fn new() -> Result<Self, ProviderError> {
        Self::with_base_url(BASE_URL)
    }

    pub fn with_base_url(base_url: &str) -> Result<Self, ProviderError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        Ok(Self { client, base_url: base_url.trim_end_matches('/').to_owned() })
    }

    async fn fetch(&self, place: &Place) -> Result<Vec<WeatherAlert>, ProviderError> {
        let url = format!("{}/alerts/active?point={:.4},{:.4}", self.base_url, place.latitude, place.longitude);
        let resp = self
            .client
            .get(&url)
            .header("accept", "application/geo+json")
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| ProviderError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(parse_problem(status.as_u16(), &body));
        }
        parse_alerts(&body, Utc::now())
    }
}

impl WeatherAlertsProvider for Nws {
    fn active<'a>(&'a self, place: &'a Place) -> BoxFuture<'a, Result<Vec<WeatherAlert>, ProviderError>> {
        Box::pin(self.fetch(place))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits the real API; run with `cargo test -p marqueet-provider-nws -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_alerts() {
        let nws = Nws::new().unwrap();
        let kc = Place { name: "Kansas City".into(), latitude: 39.0997, longitude: -94.5786, time_zone: None };
        println!("KC: {:?}", nws.active(&kc).await.unwrap());
        let oslo = Place { name: "Oslo".into(), latitude: 59.91, longitude: 10.75, time_zone: None };
        assert!(matches!(nws.active(&oslo).await, Err(ProviderError::Unsupported(_))));
    }
}
