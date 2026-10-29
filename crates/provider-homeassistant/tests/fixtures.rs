//! Parsing tests against made-up Home Assistant responses (see fixtures/README.md).
#![allow(clippy::unwrap_used)]

use marqueet_core::camera::Camera;
use marqueet_core::provider::ProviderError;
use marqueet_provider_homeassistant::parse_cameras;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

#[test]
fn cameras_by_name() {
    let cameras = parse_cameras(&fixture("states")).unwrap();
    let cam = |id: &str, name: &str| Camera { id: id.into(), name: name.into() };
    assert_eq!(
        cameras,
        [
            cam("camera.driveway_high", "Driveway"),
            cam("camera.front_porch", "Front Porch"),
            // No friendly name: made from the id.
            cam("camera.garage_side", "Garage side"),
        ],
        "unavailable cameras and other entities are left out"
    );
}

#[test]
fn not_a_list_is_a_parse_error() {
    assert!(matches!(parse_cameras(r#"{"message": "API running."}"#), Err(ProviderError::Parse(_))));
    assert_eq!(parse_cameras("[]").unwrap(), []);
}
