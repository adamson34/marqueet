//! Weather from [Open-Meteo](https://open-meteo.com): free, no API key,
//! data under CC BY 4.0 (credited on the weather widget and in the README).
//!
//! Only the configured place's coordinates are sent, and only while a
//! weather widget is showing. Parsing is pure and tested against saved
//! responses; the HTTP client is thin.

use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use marqueet_core::provider::{BoxFuture, ProviderError, WeatherProvider};
use marqueet_core::weather::{Current, Day, Place, Units, Weather};
use serde::Deserialize;

pub const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
pub const SEARCH_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
pub const USER_AGENT: &str =
    concat!("marqueet/", env!("CARGO_PKG_VERSION"), " (+https://github.com/adamson34/marqueet)");

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ForecastBody {
    current: Option<CurrentBody>,
    daily: Option<DailyBody>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CurrentBody {
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    relative_humidity_2m: Option<f64>,
    weather_code: Option<f64>,
    wind_speed_10m: Option<f64>,
    is_day: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct DailyBody {
    time: Vec<String>,
    weather_code: Vec<Option<f64>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    precipitation_probability_max: Vec<Option<f64>>,
}

/// Parses a forecast response (see [`OpenMeteo::forecast`] for the fields
/// requested).
pub fn parse_forecast(
    body: &str,
    place: &Place,
    units: Units,
    fetched_at: DateTime<Utc>,
) -> Result<Weather, ProviderError> {
    let parsed: ForecastBody = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let c = parsed.current.ok_or_else(|| ProviderError::Parse("no current conditions".into()))?;
    let temperature = c.temperature_2m.ok_or_else(|| ProviderError::Parse("no current temperature".into()))?;
    let code = |v: Option<f64>| v.map_or(3, |v| v.clamp(0.0, 999.0) as u16);
    let current = Current {
        temperature: temperature as f32,
        feels_like: c.apparent_temperature.unwrap_or(temperature) as f32,
        humidity: c.relative_humidity_2m.unwrap_or(0.0).clamp(0.0, 100.0) as u8,
        wind: c.wind_speed_10m.unwrap_or(0.0).max(0.0) as f32,
        code: code(c.weather_code),
        is_day: c.is_day.is_none_or(|d| d > 0.5),
    };
    let d = parsed.daily.unwrap_or_default();
    let days = d
        .time
        .iter()
        .enumerate()
        .filter_map(|(i, date)| {
            let at = |v: &Vec<Option<f64>>| v.get(i).copied().flatten();
            Some(Day {
                date: NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?,
                high: at(&d.temperature_2m_max)? as f32,
                low: at(&d.temperature_2m_min)? as f32,
                code: code(at(&d.weather_code)),
                precipitation: at(&d.precipitation_probability_max).map(|p| p.clamp(0.0, 100.0) as u8),
            })
        })
        .collect();
    Ok(Weather { place: place.clone(), units, current, days, fetched_at })
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchBody {
    results: Vec<SearchResult>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SearchResult {
    name: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    admin1: Option<String>,
    country: Option<String>,
    country_code: Option<String>,
}

/// Parses a place search response. Names read "Kansas City, Missouri", with
/// the country added outside the US ("Oslo, Norway").
pub fn parse_search(body: &str) -> Result<Vec<Place>, ProviderError> {
    let parsed: SearchBody = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    Ok(parsed
        .results
        .into_iter()
        .filter_map(|r| {
            let mut parts = vec![r.name.clone()];
            if let Some(a) = r.admin1.filter(|a| !a.is_empty() && *a != r.name) {
                parts.push(a);
            }
            if r.country_code.as_deref() != Some("US")
                && let Some(c) = r.country.filter(|c| !c.is_empty())
                && !parts.contains(&c)
            {
                parts.push(c);
            }
            Some(Place { name: parts.join(", "), latitude: r.latitude?, longitude: r.longitude? })
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct OpenMeteo {
    client: reqwest::Client,
    forecast_url: String,
    search_url: String,
}

impl OpenMeteo {
    pub fn new() -> Result<Self, ProviderError> {
        Self::with_urls(FORECAST_URL, SEARCH_URL)
    }

    /// Points the provider at other servers (tests, self-hosted Open-Meteo).
    pub fn with_urls(forecast_url: &str, search_url: &str) -> Result<Self, ProviderError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        Ok(Self { client, forecast_url: forecast_url.into(), search_url: search_url.into() })
    }

    async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<String, ProviderError> {
        // Built in its own scope: the serializer isn't Send.
        let url = {
            let mut qs = form_urlencoded::Serializer::new(String::new());
            for (k, v) in query {
                qs.append_pair(k, v);
            }
            format!("{url}?{}", qs.finish())
        };
        let resp = self.client.get(&url).send().await.map_err(|e| ProviderError::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(ProviderError::Status(status.as_u16()));
        }
        resp.text().await.map_err(|e| ProviderError::Http(e.to_string()))
    }

    async fn fetch_forecast(&self, place: &Place, units: Units) -> Result<Weather, ProviderError> {
        let (temp, wind) = match units {
            Units::Fahrenheit => ("fahrenheit", "mph"),
            Units::Celsius => ("celsius", "kmh"),
        };
        let query = [
            ("latitude", format!("{:.4}", place.latitude)),
            ("longitude", format!("{:.4}", place.longitude)),
            (
                "current",
                "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day".into(),
            ),
            ("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max".into()),
            ("temperature_unit", temp.into()),
            ("wind_speed_unit", wind.into()),
            ("timezone", "auto".into()),
            ("forecast_days", "5".into()),
        ];
        let body = self.get(&self.forecast_url, &query).await?;
        parse_forecast(&body, place, units, Utc::now())
    }

    async fn fetch_search(&self, name: &str) -> Result<Vec<Place>, ProviderError> {
        let query = [
            ("name", name.trim().to_owned()),
            ("count", "5".into()),
            ("language", "en".into()),
            ("format", "json".into()),
        ];
        parse_search(&self.get(&self.search_url, &query).await?)
    }
}

impl WeatherProvider for OpenMeteo {
    fn forecast<'a>(&'a self, place: &'a Place, units: Units) -> BoxFuture<'a, Result<Weather, ProviderError>> {
        Box::pin(self.fetch_forecast(place, units))
    }

    fn search<'a>(&'a self, query: &'a str) -> BoxFuture<'a, Result<Vec<Place>, ProviderError>> {
        Box::pin(self.fetch_search(query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits the real API; run with `cargo test -p marqueet-provider-openmeteo -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_search_and_forecast() {
        let p = OpenMeteo::new().unwrap();
        let places = p.search("Kansas City").await.unwrap();
        let w = p.forecast(&places[0], Units::Fahrenheit).await.unwrap();
        println!("{}: {}° code {}, {} days", w.place.name, w.current.temperature, w.current.code, w.days.len());
        assert_eq!(w.days.len(), 5);
    }
}
