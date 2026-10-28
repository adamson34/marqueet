//! Camera snapshots from Home Assistant (ADR-0016): the `camera.*` entities
//! (`GET /api/states`) and a camera's latest picture
//! (`GET /api/camera_proxy/<entity>`), with a long-lived access token.
//!
//! The token goes only to the address the owner typed: redirects are not
//! followed, so it can't be sent on anywhere else. Parsing is pure and tested
//! against made-up responses in `tests/fixtures`.

use std::time::Duration;

use marqueet_core::camera::{Camera, is_camera_id};
use marqueet_core::provider::{BoxFuture, CameraProvider, CameraSource, EncodedImage, ProviderError};
use serde::Deserialize;

/// Biggest snapshot accepted (a 4K JPEG is well under this).
pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct State {
    entity_id: String,
    state: String,
    attributes: Attributes,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Attributes {
    friendly_name: Option<String>,
}

/// The cameras in a `/api/states` response, by name. Unavailable ones are
/// left out, and so is anything whose id couldn't safely go in a path.
pub fn parse_cameras(body: &str) -> Result<Vec<Camera>, ProviderError> {
    let states: Vec<State> = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let mut cameras: Vec<Camera> = states
        .into_iter()
        .filter(|s| is_camera_id(&s.entity_id) && s.state != "unavailable")
        .map(|s| {
            let name = s.attributes.friendly_name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| {
                // camera.front_porch -> "Front porch"
                let bare = s.entity_id.trim_start_matches("camera.").replace('_', " ");
                let mut chars = bare.chars();
                chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
            });
            Camera { id: s.entity_id, name: name.trim().to_owned() }
        })
        .collect();
    cameras.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    Ok(cameras)
}

/// What a failed request means, in words for the admin page.
pub fn describe_status(status: u16) -> ProviderError {
    match status {
        401 | 403 => ProviderError::Http("Home Assistant refused the token (make a new long-lived one)".into()),
        404 => ProviderError::NotFound("that camera".into()),
        s => ProviderError::Status(s),
    }
}

#[derive(Debug, Clone)]
pub struct HomeAssistant {
    client: reqwest::Client,
}

impl HomeAssistant {
    pub fn new() -> Result<Self, ProviderError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .user_agent(concat!("marqueet/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            // The token must not follow a redirect to another host.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Http(e.to_string()))?;
        Ok(Self { client })
    }

    async fn get(&self, source: &CameraSource, path: &str) -> Result<reqwest::Response, ProviderError> {
        let url = format!("{}{path}", source.url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&source.token)
            .send()
            .await
            .map_err(|e| ProviderError::Http(without_url(&e)))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(describe_status(status.as_u16()));
        }
        Ok(resp)
    }

    async fn fetch_cameras(&self, source: &CameraSource) -> Result<Vec<Camera>, ProviderError> {
        let body =
            self.get(source, "/api/states").await?.text().await.map_err(|e| ProviderError::Http(without_url(&e)))?;
        parse_cameras(&body)
    }

    async fn fetch_snapshot(&self, source: &CameraSource, camera_id: &str) -> Result<EncodedImage, ProviderError> {
        if !is_camera_id(camera_id) {
            return Err(ProviderError::NotFound(format!("camera {camera_id:?}")));
        }
        let mut resp = self.get(source, &format!("/api/camera_proxy/{camera_id}")).await?;
        if resp.content_length().is_some_and(|n| n > MAX_SNAPSHOT_BYTES as u64) {
            return Err(ProviderError::Parse("snapshot too big".into()));
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let mut bytes = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| ProviderError::Http(without_url(&e)))? {
            if bytes.len() + chunk.len() > MAX_SNAPSHOT_BYTES {
                return Err(ProviderError::Parse("snapshot too big".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(EncodedImage { content_type, bytes })
    }
}

/// An error's text without the request URL (logs stay free of addresses).
fn without_url(e: &reqwest::Error) -> String {
    let kind = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "couldn't connect"
    } else {
        "request failed"
    };
    format!("Home Assistant: {kind}")
}

impl CameraProvider for HomeAssistant {
    fn cameras<'a>(&'a self, source: &'a CameraSource) -> BoxFuture<'a, Result<Vec<Camera>, ProviderError>> {
        Box::pin(self.fetch_cameras(source))
    }

    fn snapshot<'a>(
        &'a self,
        source: &'a CameraSource,
        camera_id: &'a str,
    ) -> BoxFuture<'a, Result<EncodedImage, ProviderError>> {
        Box::pin(self.fetch_snapshot(source, camera_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a real Home Assistant: `MARQUEET_HA_URL` and `MARQUEET_HA_TOKEN`
    /// set, then `cargo test -p marqueet-provider-homeassistant -- --ignored`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_cameras() {
        let source = CameraSource {
            url: std::env::var("MARQUEET_HA_URL").unwrap(),
            token: std::env::var("MARQUEET_HA_TOKEN").unwrap(),
        };
        let ha = HomeAssistant::new().unwrap();
        let cameras = ha.cameras(&source).await.unwrap();
        println!("{} cameras", cameras.len());
        if let Some(c) = cameras.first() {
            let shot = ha.snapshot(&source, &c.id).await.unwrap();
            println!("{}: {} bytes of {}", c.name, shot.bytes.len(), shot.content_type);
        }
    }

    #[test]
    fn statuses_in_words() {
        assert!(describe_status(401).to_string().contains("token"));
        assert!(matches!(describe_status(404), ProviderError::NotFound(_)));
        assert!(matches!(describe_status(502), ProviderError::Status(502)));
    }
}
