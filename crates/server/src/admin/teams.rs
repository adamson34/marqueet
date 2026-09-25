//! "Your team colors and logos" on the admin page: add art for one team,
//! remove it, import a team pack, download one. Marqueet ships no logos;
//! everything here is what the person adds, kept on the device.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, LOCATION};
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use marqueet_core::Rgb;
use marqueet_core::sports::{TeamColors, TeamId};
use marqueet_core::team_art::{PackTeam, TakeoverWords, TeamArt, WORD_PLAYS};

use super::multipart::{self, Field};
use super::page::Notice;
use super::{admin_page, auth, deny, html, known_teams, pairs};
use crate::team_art::{MAX_PACK_BYTES, decode_png, encode_png, export_pack, import_pack};
use crate::web::AppState;

pub fn routes() -> Router<AppState> {
    // Uploads carry a logo or a whole pack; allow a little over the largest.
    let limit = DefaultBodyLimit::max(MAX_PACK_BYTES + 1024 * 1024);
    Router::new()
        .route("/admin/teams", post(save).layer(limit))
        .route("/admin/teams/import", post(import).layer(limit))
        .route("/admin/teams/remove", post(remove))
        .route("/admin/teams/pack.json", get(download))
        .route("/admin/teams/logo", get(logo))
}

/// A query parameter's value.
fn query(uri: &Uri, key: &str) -> Option<String> {
    form_urlencoded::parse(uri.query()?.as_bytes()).find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

/// The page again with an error, after a failed upload.
fn failed(state: &AppState, peer: SocketAddr, headers: &HeaderMap, e: String) -> Response {
    html(StatusCode::BAD_REQUEST, admin_page(state, Notice::Error(e), peer, headers))
}

fn saved() -> Response {
    (StatusCode::SEE_OTHER, [(LOCATION, "/admin?saved")]).into_response()
}

/// Access checks shared by every post here.
fn check_post(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    if !auth::same_origin(headers) {
        return Some(html(StatusCode::FORBIDDEN, "cross-site form post refused".into()));
    }
    deny(state.auth.check(peer.ip(), headers))
}

fn form_fields(headers: &HeaderMap, body: &[u8]) -> Result<Vec<Field>, String> {
    let kind = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let boundary = multipart::boundary(kind).ok_or("that form didn't send a file upload")?;
    multipart::parse(body, &boundary)
}

fn label(state: &AppState, team: &TeamId) -> Option<String> {
    state.hub.with_store(known_teams).into_iter().find(|t| &t.id == team).map(|t| t.name)
}

/// The art for one team after the form: new colors (when "use my colors"
/// is ticked), a new logo (when a file was chosen), or the old logo kept.
fn apply(current: Option<TeamArt>, label: String, fields: &[Field]) -> Result<TeamArt, String> {
    let text = |name: &str| multipart::get(fields, name).map(Field::text).unwrap_or_default();
    let color = |name: &str, what: &str| -> Result<Rgb, String> {
        text(name).trim().parse::<Rgb>().map_err(|_| format!("{what} isn't a color"))
    };
    let colors = if text("custom_colors") == "on" {
        Some(TeamColors {
            primary: color("primary", "The main color")?,
            secondary: Some(color("secondary", "The second color")?),
        })
    } else {
        None
    };
    let (old_logo, old_words, old_art) = current.map(|c| (c.logo, c.words, c.art)).unwrap_or_default();
    let logo = match multipart::get(fields, "logo").filter(|f| !f.data.is_empty()) {
        Some(file) => Some(decode_png(&file.data)?),
        None if text("remove_logo") == "on" => None,
        None => old_logo,
    };
    // Takeover words: a headline typed for a play replaces that play's
    // words; left blank, what was there stays.
    let mut words = if text("remove_words") == "on" { Default::default() } else { old_words };
    for (play, ..) in WORD_PLAYS {
        let headline = text(&format!("words_{play}"));
        if let Some((play, w)) = TakeoverWords::clean(play, &headline, &text(&format!("words_{play}_line"))) {
            words.insert(play, w);
        }
    }
    // LED art: a new file replaces it (with its frame count, speed and
    // placement); otherwise what's there stays.
    let art = match multipart::get(fields, "art").filter(|f| !f.data.is_empty()) {
        Some(file) => {
            let strip: u32 = text("art_frames").trim().parse().unwrap_or(1);
            let (frames, file_ms) = marqueet_core::art::decode(&file.data, strip)?;
            let ms =
                text("art_ms").trim().parse::<u16>().ok().or(file_ms).unwrap_or(marqueet_core::art::FRAME_MS_DEFAULT);
            let placement = marqueet_core::art::ArtPlacement::from_id(text("art_placement").trim()).unwrap_or_default();
            Some(marqueet_core::art::TakeoverArt::new(frames, ms, placement)?)
        }
        None if text("remove_art") == "on" => None,
        None => old_art,
    };
    Ok(TeamArt { label, colors, logo, words, art })
}

async fn save(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = check_post(&state, peer, &headers) {
        return denied;
    }
    let result = (|| {
        let fields = form_fields(&headers, &body)?;
        let team = multipart::get(&fields, "team").map(Field::text).unwrap_or_default();
        if team.trim().is_empty() {
            return Err("pick a team first".to_owned());
        }
        let team = TeamId(team.trim().to_owned());
        let name = label(&state, &team).unwrap_or_else(|| team.0.clone());
        let art = apply(state.hub.team_art().get(&team).cloned(), name, &fields)?;
        if art.is_empty() { state.hub.remove_team_art(&team) } else { state.hub.set_team_art(vec![(team, art)]) }
    })();
    match result {
        Ok(()) => saved(),
        Err(e) => failed(&state, peer, &headers, e),
    }
}

async fn import(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = check_post(&state, peer, &headers) {
        return denied;
    }
    let teams = state.hub.with_store(known_teams);
    let request_headers = headers.clone();
    // Decoding hundreds of PNGs is slow; keep it off the async threads.
    let decoded = tokio::task::spawn_blocking(move || {
        let fields = form_fields(&request_headers, &body)?;
        let file =
            multipart::get(&fields, "pack").filter(|f| !f.data.is_empty()).ok_or("choose a team pack file first")?;
        import_pack(&file.data, |id| teams.iter().find(|t| &t.id == id).map(|t| t.name.clone()))
    })
    .await
    .unwrap_or_else(|e| Err(e.to_string()));
    let result = decoded.and_then(|entries| {
        if entries.is_empty() {
            return Err("that team pack has no colors or logos in it".to_owned());
        }
        state.hub.set_team_art(entries)
    });
    match result {
        Ok(()) => saved(),
        Err(e) => failed(&state, peer, &headers, e),
    }
}

async fn remove(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = check_post(&state, peer, &headers) {
        return denied;
    }
    let team = pairs(&body).into_iter().find(|(k, _)| k == "team").map(|(_, v)| v).unwrap_or_default();
    match state.hub.remove_team_art(&TeamId(team)) {
        Ok(()) => saved(),
        Err(e) => failed(&state, peer, &headers, e),
    }
}

/// `GET /admin/teams/pack.json`: your team pack; with `?all=1`, plus every
/// team you follow (with today's colors where known) to fill in.
async fn download(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let all = query(&uri, "all").is_some();
    if let Some(denied) = deny(state.auth.check(peer.ip(), &headers)) {
        return denied;
    }
    let art = state.hub.team_art();
    let extra: Vec<(TeamId, String, Option<PackTeam>)> = if all {
        state.hub.with_store(|store| {
            let games = store.games();
            known_teams(store)
                .into_iter()
                .map(|t| {
                    let colors = games
                        .iter()
                        .flat_map(|g| [&g.away, &g.home])
                        .find(|c| c.team.id == t.id)
                        .map(|c| c.team.colors);
                    let data = colors.map(|c| PackTeam {
                        primary: Some(c.primary),
                        secondary: c.secondary,
                        ..PackTeam::default()
                    });
                    (t.id, t.name, data)
                })
                .collect()
        })
    } else {
        Vec::new()
    };
    let json = serde_json::to_string_pretty(&export_pack(&art, &extra)).unwrap_or_default();
    let name = if all { "marqueet-teams-template.json" } else { "marqueet-teams.json" };
    let headers = [
        (CONTENT_TYPE, "application/json".to_owned()),
        (CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        (CACHE_CONTROL, "no-store".to_owned()),
    ];
    (headers, json).into_response()
}

/// `GET /admin/teams/logo?team=...`: a team's logo, for the admin page.
async fn logo(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let team = query(&uri, "team").unwrap_or_default();
    if let Some(denied) = deny(state.auth.check(peer.ip(), &headers)) {
        return denied;
    }
    match state.hub.team_art().get(&TeamId(team)).and_then(|a| a.logo.clone()) {
        Some(image) => ([(CONTENT_TYPE, "image/png"), (CACHE_CONTROL, "no-store")], encode_png(&image)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::team_art::Image;

    fn field(name: &str, value: &str) -> Field {
        Field { name: name.into(), filename: None, data: value.as_bytes().to_vec() }
    }

    #[test]
    fn colors_only_when_ticked() {
        let fields = [field("primary", "#112233"), field("secondary", "#445566")];
        assert_eq!(apply(None, "x".into(), &fields).unwrap().colors, None, "unticked keeps the data's colors");
        let ticked = [field("custom_colors", "on"), field("primary", "#112233"), field("secondary", "#445566")];
        let art = apply(None, "x".into(), &ticked).unwrap();
        assert_eq!(art.colors.unwrap().primary, Rgb::new(0x11, 0x22, 0x33));
        let bad = [field("custom_colors", "on"), field("primary", "blurple"), field("secondary", "#445566")];
        assert!(apply(None, "x".into(), &bad).unwrap_err().contains("main color"));
    }

    /// A PNG strip of `n` 8x4 frames.
    fn strip_png(n: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, 8 * n, 4);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&vec![200; (8 * n * 4 * 4) as usize]).unwrap();
        out
    }

    #[test]
    fn led_art_is_uploaded_kept_or_removed() {
        use marqueet_core::art::ArtPlacement;
        let file = Field { name: "art".into(), filename: Some("fw.png".into()), data: strip_png(3) };
        let set = apply(
            None,
            "x".into(),
            &[file, field("art_frames", "3"), field("art_ms", "120"), field("art_placement", "intro")],
        )
        .unwrap();
        let art = set.art.clone().unwrap();
        assert_eq!((art.frames.len(), art.size(), art.frame_ms, art.placement), (3, (8, 4), 120, ArtPlacement::Intro));
        assert!(!set.is_empty(), "art alone is worth keeping");
        let kept = apply(Some(set.clone()), "x".into(), &[field("art_placement", "above")]).unwrap();
        assert_eq!(kept.art, set.art, "no new file: the art stays as it was");
        let gone = apply(Some(set), "x".into(), &[field("remove_art", "on")]).unwrap();
        assert!(gone.art.is_none());
        let bad = Field { name: "art".into(), filename: Some("x.png".into()), data: strip_png(3) };
        assert!(apply(None, "x".into(), &[bad, field("art_frames", "5")]).unwrap_err().contains("doesn't split"));
    }

    #[test]
    fn takeover_words_are_kept_replaced_or_removed() {
        let set = apply(
            None,
            "x".into(),
            &[field("words_touchdown", "kingdom td!"), field("words_touchdown_line", "hear it")],
        )
        .unwrap();
        assert_eq!(set.words["touchdown"].headline, "KINGDOM TD!");
        assert!(!set.is_empty(), "words alone are worth keeping");
        let kept = apply(Some(set.clone()), "x".into(), &[field("words_touchdown", "")]).unwrap();
        assert_eq!(kept.words, set.words, "blank keeps them");
        let more = apply(Some(set.clone()), "x".into(), &[field("words_goal", "goal!")]).unwrap();
        assert_eq!(more.words.len(), 2);
        let gone = apply(Some(set), "x".into(), &[field("remove_words", "on")]).unwrap();
        assert!(gone.words.is_empty());
    }

    #[test]
    fn logos_are_kept_replaced_or_removed() {
        let old = TeamArt {
            label: "x".into(),
            colors: None,
            logo: Image::new(1, 1, vec![1, 2, 3, 4]),
            words: Default::default(),
            art: Default::default(),
        };
        let empty_file = Field { name: "logo".into(), filename: Some(String::new()), data: vec![] };
        let kept = apply(Some(old.clone()), "x".into(), std::slice::from_ref(&empty_file)).unwrap();
        assert_eq!(kept.logo, old.logo, "no file chosen keeps the logo");
        let removed = apply(Some(old.clone()), "x".into(), &[empty_file, field("remove_logo", "on")]).unwrap();
        assert_eq!(removed.logo, None);
        let not_png = Field { name: "logo".into(), filename: Some("a.gif".into()), data: b"GIF89a".to_vec() };
        assert!(apply(Some(old), "x".into(), &[not_png]).unwrap_err().contains("PNG"));
    }
}
