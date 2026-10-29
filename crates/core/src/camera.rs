//! Cameras from a camera source (Home Assistant, ADR-0016): what's on offer,
//! how a view names a camera's latest snapshot, and turning a snapshot's
//! bytes into a picture small enough to send to displays. Pure.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::team_art::Image;

/// A camera the source offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Camera {
    /// The source's id for it (Home Assistant: `camera.front_porch`).
    pub id: String,
    /// What people call it ("Front Porch").
    pub name: String,
}

/// Longest side of a snapshot sent to displays; bigger ones are shrunk to
/// fit, keeping their shape.
pub const SNAPSHOT_MAX: u32 = 960;

/// Widest or tallest picture decoded at all (8K); anything bigger is refused
/// before its pixels are allocated.
const DECODE_MAX_SIDE: usize = 7680;

/// The latest picture of each camera a display has, by snapshot key.
pub type Snapshots = HashMap<String, Image>;

/// Fastest a camera is fetched again, in seconds.
pub const MIN_REFRESH_SECS: u16 = 2;

/// The key a view uses for a camera's snapshot. Displays treat it as an
/// opaque name (ADR-0003).
pub fn snapshot_key(camera_id: &str) -> String {
    format!("camera:{camera_id}")
}

/// The camera a snapshot key names (the reverse of [`snapshot_key`]).
pub fn camera_of(key: &str) -> Option<&str> {
    key.strip_prefix("camera:").filter(|id| is_camera_id(id))
}

/// A snapshot's bytes (JPEG or PNG) as a picture at most [`SNAPSHOT_MAX`] on
/// its longest side.
pub fn decode_snapshot(bytes: &[u8]) -> Result<Image, String> {
    let image = if bytes.starts_with(&[0xff, 0xd8]) {
        decode_jpeg(bytes)?
    } else if bytes.starts_with(b"\x89PNG") {
        crate::team_art::decode_png(bytes, DECODE_MAX_SIDE as u32)?
    } else {
        return Err("the camera sent something that isn't a JPEG or PNG".into());
    };
    Ok(image.fit(SNAPSHOT_MAX))
}

fn decode_jpeg(bytes: &[u8]) -> Result<Image, String> {
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(DECODE_MAX_SIDE)
        .set_max_height(DECODE_MAX_SIDE);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let rgba = decoder.decode().map_err(|e| format!("couldn't read that JPEG ({e})"))?;
    let (w, h) = decoder.dimensions().ok_or("couldn't read that JPEG's size")?;
    Image::new(w as u32, h as u32, rgba).ok_or_else(|| "couldn't read that JPEG".to_owned())
}

/// True for an id that looks like a camera's (letters, digits, `_` and `.`),
/// so ids from forms and feeds can be put in a request path safely.
pub fn is_camera_id(id: &str) -> bool {
    ("camera.".len() + 1..=255).contains(&id.len())
        && id.starts_with("camera.")
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_ids() {
        assert!(is_camera_id("camera.front_porch"));
        assert!(is_camera_id("camera.driveway_2"));
        for bad in ["", "camera.", "light.porch", "camera.porch/../states", "camera.Porch", "camera.a b"] {
            assert!(!is_camera_id(bad), "{bad:?}");
        }
        assert_eq!(snapshot_key("camera.front_porch"), "camera:camera.front_porch");
        assert_eq!(camera_of("camera:camera.front_porch"), Some("camera.front_porch"));
        assert_eq!(camera_of("camera:light.porch"), None);
        assert_eq!(camera_of("espn:nfl:1"), None);
    }

    /// A made-up 64x48 picture (a gradient), as a camera would send it.
    const JPEG: &[u8] = include_bytes!("../tests/fixtures/snapshot.jpg");

    #[test]
    fn snapshots_decode_and_shrink() {
        let image = decode_snapshot(JPEG).unwrap();
        assert_eq!((image.width, image.height), (64, 48));
        assert!(image.rgba.chunks(4).all(|p| p[3] == 255), "opaque");
        let big = Image::new(1920, 1080, vec![90; 1920 * 1080 * 4]).unwrap();
        let png = {
            let mut out = Vec::new();
            let mut e = png::Encoder::new(&mut out, 1920, 1080);
            e.set_color(png::ColorType::Rgba);
            e.write_header().unwrap().write_image_data(&big.rgba).unwrap();
            out
        };
        let shrunk = decode_snapshot(&png).unwrap();
        assert_eq!((shrunk.width, shrunk.height), (SNAPSHOT_MAX, 540), "keeps its shape");
        assert!(decode_snapshot(b"<html>").is_err());
        assert!(decode_snapshot(&[0xff, 0xd8, 0xff]).is_err(), "truncated");
    }
}
