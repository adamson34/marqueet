//! Reading and writing people's team art: PNG logos and team pack files
//! (see `marqueet_core::team_art`). Marqueet ships no logos; these only
//! handle what someone adds.

use std::io::Cursor;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use marqueet_core::sports::TeamId;
use marqueet_core::team_art::{Image, LOGO_MAX, PACK_VERSION, PackTeam, TeamArt, TeamArtMap, TeamPack};

/// Largest logo file accepted.
pub const MAX_LOGO_BYTES: usize = 2 * 1024 * 1024;
/// Largest team pack file accepted.
pub const MAX_PACK_BYTES: usize = 24 * 1024 * 1024;
/// Most teams in one pack.
pub const MAX_PACK_TEAMS: usize = 600;
/// Largest image side decoded (bigger files are refused, not shrunk).
const MAX_SIDE: u32 = 2048;

/// Decodes a PNG and scales it to fit [`LOGO_MAX`].
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    if bytes.len() > MAX_LOGO_BYTES {
        return Err("that logo is too big; use a PNG under 2 MB".into());
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("logos must be PNG files".into());
    }
    let bad = |e: png::DecodingError| format!("couldn't read that PNG ({e})");
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(bad)?;
    let (w, h) = (reader.info().width, reader.info().height);
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(format!("that logo is {w}x{h}; use one at most {MAX_SIDE} pixels on a side"));
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or("that PNG is too large")?];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    let data = &buf[..frame.buffer_size()];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("couldn't read that PNG's colors".into()),
    };
    let image = Image::new(frame.width, frame.height, rgba).ok_or("couldn't read that PNG")?;
    Ok(image.fit(LOGO_MAX))
}

/// Encodes an image as PNG (logo previews, pack export).
pub fn encode_png(image: &Image) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    // Writing into memory can't fail for a valid image.
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(&image.rgba);
    }
    out
}

/// Reads a team pack file into team art, checking every logo and color.
/// `label` names a team we know (for teams listed without a name).
pub fn import_pack(json: &[u8], label: impl Fn(&TeamId) -> Option<String>) -> Result<Vec<(TeamId, TeamArt)>, String> {
    if json.len() > MAX_PACK_BYTES {
        return Err("that team pack is too big (24 MB at most)".into());
    }
    let pack: TeamPack =
        serde_json::from_slice(json).map_err(|e| format!("that doesn't look like a Marqueet team pack ({e})"))?;
    if pack.marqueet_team_pack != PACK_VERSION {
        return Err(format!(
            "that team pack is version {}; this Marqueet reads version {PACK_VERSION}",
            pack.marqueet_team_pack
        ));
    }
    if pack.teams.len() > MAX_PACK_TEAMS {
        return Err(format!("that team pack has {} teams; {MAX_PACK_TEAMS} at most", pack.teams.len()));
    }
    pack.teams
        .into_iter()
        .filter(|t| !t.team.0.trim().is_empty())
        .map(|t| {
            let logo = match &t.logo_png {
                Some(b64) => {
                    let bytes = STANDARD
                        .decode(b64.trim().as_bytes())
                        .map_err(|_| format!("{}: the logo isn't valid base64", t.team.0))?;
                    Some(decode_png(&bytes).map_err(|e| format!("{}: {e}", t.team.0))?)
                }
                None => None,
            };
            let name = if t.name.trim().is_empty() {
                label(&t.team).unwrap_or_else(|| t.team.0.clone())
            } else {
                t.name.trim().to_owned()
            };
            let words = marqueet_core::team_art::clean_words(&t.takeovers);
            Ok((t.team.clone(), TeamArt { label: name, colors: t.colors(), logo, words }))
        })
        .filter(|r| !matches!(r, Ok((_, a)) if a.is_empty()))
        .collect()
}

/// A team pack of `art`, plus (as a starting point for making one) every
/// team in `extra` that has no art yet, with its colors from the data.
pub fn export_pack(art: &TeamArtMap, extra: &[(TeamId, String, Option<PackTeam>)]) -> TeamPack {
    let mut teams: Vec<PackTeam> = art
        .iter()
        .map(|(team, a)| PackTeam {
            team: team.clone(),
            name: a.label.clone(),
            primary: a.colors.map(|c| c.primary),
            secondary: a.colors.and_then(|c| c.secondary),
            logo_png: a.logo.as_ref().map(|l| STANDARD.encode(encode_png(l))),
            takeovers: a.words.clone(),
        })
        .collect();
    for (team, name, data) in extra {
        if !art.contains_key(team) {
            let mut t = data.clone().unwrap_or_default();
            t.team = team.clone();
            t.name.clone_from(name);
            teams.push(t);
        }
    }
    TeamPack { marqueet_team_pack: PACK_VERSION, teams }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::Rgb;
    use marqueet_core::sports::TeamColors;

    fn png_bytes(w: u32, h: u32, color: png::ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        e.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    #[test]
    fn pngs_of_every_kind_decode_and_shrink() {
        let rgba = decode_png(&png_bytes(2, 1, png::ColorType::Rgba, &[255, 0, 0, 255, 0, 0, 0, 0])).unwrap();
        assert_eq!(rgba.rgba, vec![255, 0, 0, 255, 0, 0, 0, 0]);
        let rgb = decode_png(&png_bytes(1, 1, png::ColorType::Rgb, &[1, 2, 3])).unwrap();
        assert_eq!(rgb.rgba, vec![1, 2, 3, 255]);
        let gray = decode_png(&png_bytes(1, 1, png::ColorType::Grayscale, &[9])).unwrap();
        assert_eq!(gray.rgba, vec![9, 9, 9, 255]);
        let big = decode_png(&png_bytes(512, 256, png::ColorType::Rgb, &vec![200; 512 * 256 * 3])).unwrap();
        assert_eq!((big.width, big.height), (LOGO_MAX, LOGO_MAX / 2));
    }

    #[test]
    fn non_pngs_and_huge_images_are_refused() {
        assert!(decode_png(b"GIF89a....").unwrap_err().contains("PNG"));
        assert!(decode_png(b"\x89PNG\r\n\x1a\nnope").is_err());
        let huge = png_bytes(5000, 1, png::ColorType::Grayscale, &vec![0; 5000]);
        assert!(decode_png(&huge).unwrap_err().contains("5000x1"));
    }

    #[test]
    fn pngs_round_trip() {
        let img = Image::new(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!(decode_png(&encode_png(&img)).unwrap(), img);
    }

    #[test]
    fn packs_import_what_they_export() {
        let team = TeamId("espn:nfl:12".into());
        let art: TeamArtMap = [(
            team.clone(),
            TeamArt {
                label: "Somewhere".into(),
                colors: Some(TeamColors { primary: Rgb::RED, secondary: Some(Rgb::WHITE) }),
                logo: Image::new(1, 1, vec![10, 20, 30, 255]),
                words: [(
                    "touchdown".to_owned(),
                    marqueet_core::team_art::TakeoverWords { headline: "KINGDOM TD!".into(), line: None },
                )]
                .into(),
            },
        )]
        .into();
        let other = (TeamId("espn:nfl:2".into()), "Elsewhere".into(), None);
        let pack = export_pack(&art, &[other]);
        assert_eq!(pack.teams.len(), 2, "art plus a blank entry to fill in");
        let json = serde_json::to_vec(&pack).unwrap();
        let back = import_pack(&json, |_| None).unwrap();
        assert_eq!(back, vec![(team.clone(), art[&team].clone())], "the blank entry adds nothing");
    }

    #[test]
    fn bad_packs_say_why() {
        assert!(import_pack(b"{}", |_| None).unwrap_err().contains("team pack"));
        assert!(import_pack(br#"{"marqueet_team_pack":9,"teams":[]}"#, |_| None).unwrap_err().contains("version 9"));
        let bad_logo = br#"{"marqueet_team_pack":1,"teams":[{"team":"x","logo_png":"%%%"}]}"#;
        assert!(import_pack(bad_logo, |_| None).unwrap_err().starts_with("x: "));
        let named = br##"{"marqueet_team_pack":1,"teams":[{"team":"x","primary":"#123456"}]}"##;
        let got = import_pack(named, |_| Some("Known Name".into())).unwrap();
        assert_eq!(got[0].1.label, "Known Name");
    }
}
