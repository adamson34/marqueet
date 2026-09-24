//! The welcome flow: right after first-boot setup, four short, friendly
//! steps (sports, teams, town, fantasy) instead of the full admin page.
//! Every step saves through the same settings path as the admin page, and
//! the town and fantasy steps can be skipped.

use std::fmt::Write as _;
use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use marqueet_core::provider::LeagueInfo;
use marqueet_core::sports::{LeagueId, TeamId};
use marqueet_core::weather::Units;

use super::page::{checked, esc, head};
use super::{admin_form, deny, field, form, html, pairs, redirect};
use crate::hub::{FantasySearch, Hub};
use crate::web::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/welcome", get(|| async { redirect("/welcome/sports") }))
        .route("/welcome/sports", get(sports_page).post(save_sports))
        .route("/welcome/teams", get(teams_page).post(save_teams))
        .route("/welcome/town", get(town_page).post(save_town))
        .route("/welcome/fantasy", get(fantasy_page).post(find_fantasy))
        .route("/welcome/fantasy/add", post(add_fantasy))
        .route("/welcome/done", get(done_page))
}

const STEPS: [&str; 4] = ["Sports", "Teams", "Town", "Fantasy"];

/// A welcome page: progress, a heading, a line of help, then `body`.
fn shell(step: usize, title: &str, help: &str, error: Option<&str>, body: &str) -> String {
    let mut h = head("Welcome to Marqueet");
    h.push_str(
        "<header class=\"top\"><span class=\"brand\">MARQUEET</span><span class=\"sub\">welcome</span></header>\
         <main class=\"narrow welcome\"><ol class=\"steps\">",
    );
    for (i, name) in STEPS.iter().enumerate() {
        let class = if i + 1 == step {
            " class=\"now\""
        } else if i + 1 < step {
            " class=\"done\""
        } else {
            ""
        };
        let _ = write!(h, "<li{class}>{name}</li>");
    }
    let _ = write!(h, "</ol><h2>{}</h2><p class=\"hint\">{}</p>", esc(title), esc(help));
    if let Some(e) = error {
        let _ = write!(h, "<p class=\"notice err\" role=\"alert\">{}</p>", esc(e));
    }
    h.push_str(body);
    h.push_str("</main></body></html>");
    h
}

fn buttons(skip: Option<&str>, next: &str) -> String {
    let skip = skip.map_or(String::new(), |href| format!("<a class=\"skip\" href=\"{href}\">Skip</a>"));
    format!("<div class=\"actions\">{skip}<button class=\"primary\">{next}</button></div>")
}

pub(super) fn sports(leagues: &[LeagueInfo], following: &[LeagueId], error: Option<&str>) -> String {
    let mut b = String::from("<form method=\"post\" action=\"/welcome/sports\"><div class=\"tiles\">");
    for l in leagues {
        let _ = write!(
            b,
            "<label class=\"tile\"><input type=\"checkbox\" name=\"league\" value=\"{}\"{}><span>{}</span></label>",
            esc(l.id.as_str()),
            checked(following.contains(&l.id)),
            esc(&l.name)
        );
    }
    b.push_str("</div>");
    b.push_str(&buttons(None, "Next"));
    b.push_str("</form>");
    shell(1, "Which sports do you follow?", "Pick as many as you like. You can change this any time.", error, &b)
}

/// (league name, teams) for each followed league, with a note when a
/// league's team list hasn't loaded yet.
pub(super) type TeamGroups = Vec<(String, Vec<(TeamId, String)>)>;

pub(super) fn teams(groups: &TeamGroups, favorites: &[TeamId]) -> String {
    let mut b = String::from("<form method=\"post\" action=\"/welcome/teams\">");
    for (league, teams) in groups {
        let picked = teams.iter().filter(|(id, _)| favorites.contains(id)).count();
        let _ = write!(
            b,
            "<details class=\"teams\"{}><summary>{}<span class=\"count\">{}</span></summary><div>",
            if picked > 0 || groups.len() == 1 { " open" } else { "" },
            esc(league),
            if picked > 0 { format!("{picked} picked") } else { format!("{} teams", teams.len()) },
        );
        if teams.is_empty() {
            b.push_str("<p class=\"empty\">Still loading these teams: refresh this page in a minute.</p>");
        }
        if teams.len() > 40 {
            b.push_str(
                "<input type=\"search\" class=\"team-filter js-only\" placeholder=\"Find a team\" \
                 aria-label=\"Find a team\" autocomplete=\"off\">",
            );
        }
        for (id, name) in teams {
            let _ = write!(
                b,
                "<label><input type=\"checkbox\" name=\"favorite\" value=\"{}\"{}> {}</label>",
                esc(&id.0),
                checked(favorites.contains(id)),
                esc(name)
            );
        }
        b.push_str("</div></details>");
    }
    b.push_str(&buttons(Some("/welcome/town"), "Next"));
    b.push_str("</form>");
    shell(
        2,
        "Who are your teams?",
        "Their games come first on the ticker, and their big plays take over the screen. Tap a league to open it.",
        None,
        &b,
    )
}

pub(super) fn town(place: &str, units: Units, error: Option<&str>) -> String {
    let b = format!(
        "<form method=\"post\" action=\"/welcome/town\">\
         <label class=\"wide\">Your town or city <input name=\"location\" value=\"{}\" placeholder=\"e.g. Kansas City\" \
         autocomplete=\"off\"></label>\
         <div class=\"choices inline\"><label><input type=\"radio\" name=\"units\" value=\"fahrenheit\"{}> °F</label>\
         <label><input type=\"radio\" name=\"units\" value=\"celsius\"{}> °C</label></div>{}</form>",
        esc(place),
        checked(units == Units::Fahrenheit),
        checked(units == Units::Celsius),
        buttons(Some("/welcome/fantasy"), "Next"),
    );
    shell(
        3,
        "Where are you?",
        "For the weather on the ticker and severe weather warnings (US). Only your town is shared, with the free \
         weather services.",
        error,
        &b,
    )
}

pub(super) fn fantasy(following: &[String], search: Option<&FantasySearch>, error: Option<&str>) -> String {
    let mut b = String::new();
    if !following.is_empty() {
        b.push_str("<ul class=\"following\">");
        for f in following {
            let _ = write!(b, "<li>✓ {}</li>", esc(f));
        }
        b.push_str("</ul>");
    }
    match search {
        None => b.push_str(
            "<form method=\"post\" action=\"/welcome/fantasy\"><label class=\"wide\">Your Sleeper username \
             <input name=\"username\" required autocomplete=\"off\" spellcheck=\"false\"></label>\
             <div class=\"actions\"><a class=\"skip\" href=\"/welcome/done\">Skip</a>\
             <button class=\"primary\">Find my leagues</button></div></form>",
        ),
        Some(s) if s.leagues.is_empty() => {
            let _ = write!(b, "<p class=\"empty\">{} has no leagues this season.</p>", esc(&s.user.display_name));
        }
        Some(s) => {
            for (league, teams) in &s.leagues {
                let _ = write!(
                    b,
                    "<form method=\"post\" action=\"/welcome/fantasy/add\" class=\"row new-feed\">\
                     <input type=\"hidden\" name=\"league_id\" value=\"{}\"><input type=\"hidden\" name=\"league\" value=\"{}\">\
                     <label><strong>{}</strong> <select name=\"roster_id\">",
                    esc(&league.id),
                    esc(&league.name),
                    esc(&league.name)
                );
                for t in teams {
                    let mine = t.owner_id.as_deref() == Some(s.user.id.as_str());
                    let _ = write!(
                        b,
                        "<option value=\"{}\"{}>{}</option>",
                        t.roster_id,
                        if mine { " selected" } else { "" },
                        esc(&t.name)
                    );
                }
                b.push_str("</select></label><button>Follow</button></form>");
            }
        }
    }
    if !following.is_empty() || search.is_some() {
        b.push_str("<div class=\"actions\"><a class=\"button primary\" href=\"/welcome/done\">Finish</a></div>");
    }
    shell(
        4,
        "Play fantasy football?",
        "Follow your Sleeper league: your matchup rides on the ticker, and a touchdown by your player says so on \
         the screen.",
        error,
        &b,
    )
}

pub(super) fn done() -> String {
    shell(
        5,
        "You're all set!",
        "Look at the screen: your sports and teams are on the ticker now.",
        None,
        "<p>Change anything later at <strong>marqueet.local:7878</strong> (bookmark it), including the layout, \
         colors, and quiet hours overnight.</p><div class=\"actions\"><a class=\"button primary\" href=\"/admin\">\
         All settings</a></div>",
    )
}

async fn guard(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    deny(state.auth.check(peer.ip(), headers))
}

fn team_groups(hub: &Hub) -> TeamGroups {
    let leagues = hub.supported_leagues();
    hub.with_store(|store| {
        store
            .leagues()
            .iter()
            .map(|league| {
                let name = leagues.iter().find(|l| &l.id == league).map_or(league.as_str(), |l| l.name.as_str());
                let mut teams: Vec<(TeamId, String)> =
                    store.teams(league).iter().map(|t| (t.id.clone(), t.name.clone())).collect();
                if teams.is_empty() {
                    for g in store.games().iter().filter(|g| &g.league == league) {
                        for c in [&g.away, &g.home] {
                            if !teams.iter().any(|(id, _)| *id == c.team.id) {
                                teams.push((c.team.id.clone(), c.team.display_name.clone()));
                            }
                        }
                    }
                    teams.sort_by(|a, b| a.1.cmp(&b.1));
                }
                (name.to_owned(), teams)
            })
            .collect()
    })
}

async fn sports_page(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = guard(&state, peer, &headers).await {
        return denied;
    }
    html(StatusCode::OK, sports(&state.hub.supported_leagues(), &state.hub.settings().leagues, None))
}

async fn save_sports(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    let supported = state.hub.supported_leagues();
    let chosen: Vec<String> = pairs(&body).into_iter().filter(|(k, _)| k == "league").map(|(_, v)| v).collect();
    // Keep the provider's order (the popular leagues come first).
    let leagues: Vec<LeagueId> =
        supported.iter().filter(|l| chosen.iter().any(|c| c == l.id.as_str())).map(|l| l.id.clone()).collect();
    if leagues.is_empty() {
        let current = state.hub.settings().leagues;
        return html(StatusCode::BAD_REQUEST, sports(&supported, &current, Some("Pick at least one sport.")));
    }
    let mut settings = state.hub.settings();
    settings.leagues = leagues;
    match state.hub.apply_settings(settings) {
        Ok(_) => redirect("/welcome/teams"),
        Err(e) => html(StatusCode::BAD_REQUEST, sports(&supported, &state.hub.settings().leagues, Some(&e))),
    }
}

async fn teams_page(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = guard(&state, peer, &headers).await {
        return denied;
    }
    html(StatusCode::OK, teams(&team_groups(&state.hub), &state.hub.settings().favorites))
}

async fn save_teams(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    let mut settings = state.hub.settings();
    settings.favorites =
        pairs(&body).into_iter().filter(|(k, v)| k == "favorite" && !v.is_empty()).map(|(_, v)| TeamId(v)).collect();
    match state.hub.apply_settings(settings) {
        Ok(_) => redirect("/welcome/town"),
        Err(_) => redirect("/welcome/teams"),
    }
}

async fn town_page(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = guard(&state, peer, &headers).await {
        return denied;
    }
    let s = state.hub.settings();
    html(StatusCode::OK, town(s.weather.place.as_ref().map_or("", |p| p.name.as_str()), s.weather.units, None))
}

async fn save_town(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    let pairs = pairs(&body);
    let current = state.hub.settings();
    let mut settings = current.clone();
    settings.weather.units = if field(&body, "units") == "celsius" { Units::Celsius } else { Units::Fahrenheit };
    let result = async {
        match form::location(&pairs, current.weather.place.as_ref()) {
            form::LocationChange::Keep => {}
            form::LocationChange::Clear => settings.weather.place = None,
            form::LocationChange::Set(place) => settings.weather.place = Some(place),
            form::LocationChange::Lookup(query) => {
                let found = state.hub.search_places(&query).await?;
                let place = found
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("We couldn't find \"{query}\". Try the nearest city."))?;
                settings.weather.place = Some(place);
            }
        }
        state.hub.apply_settings(settings.clone())
    }
    .await;
    match result {
        Ok(_) => redirect("/welcome/fantasy"),
        Err(e) => {
            let typed = field(&body, "location");
            html(StatusCode::BAD_REQUEST, town(&typed, settings.weather.units, Some(&e)))
        }
    }
}

fn following(hub: &Hub) -> Vec<String> {
    hub.settings().fantasy.iter().map(|f| format!("{} ({})", f.team, f.league)).collect()
}

async fn fantasy_page(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = guard(&state, peer, &headers).await {
        return denied;
    }
    html(StatusCode::OK, fantasy(&following(&state.hub), None, None))
}

async fn find_fantasy(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    match state.hub.find_fantasy(&field(&body, "username")).await {
        Ok(search) => html(StatusCode::OK, fantasy(&following(&state.hub), Some(&search), None)),
        Err(_) => html(
            StatusCode::BAD_REQUEST,
            fantasy(&following(&state.hub), None, Some("We couldn't find that Sleeper username. Check the spelling.")),
        ),
    }
}

async fn add_fantasy(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(denied) = admin_form(&state, peer, &headers) {
        return denied;
    }
    let roster: u32 = field(&body, "roster_id").parse().unwrap_or(0);
    match state.hub.add_fantasy(&field(&body, "league_id"), &field(&body, "league"), roster).await {
        Ok(()) => redirect("/welcome/fantasy"),
        Err(e) => html(StatusCode::BAD_REQUEST, fantasy(&following(&state.hub), None, Some(&e))),
    }
}

async fn done_page(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if let Some(denied) = guard(&state, peer, &headers).await {
        return denied;
    }
    html(StatusCode::OK, done())
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::sports::Sport;

    #[test]
    fn steps_show_progress_and_escape() {
        let leagues = [LeagueInfo { id: LeagueId::new("nfl"), sport: Sport::Football, name: "NFL <pro>".into() }];
        let page = sports(&leagues, &[LeagueId::new("nfl")], Some("Pick <one>"));
        assert!(page.contains("<li class=\"now\">Sports</li><li>Teams</li>"));
        assert!(page.contains("value=\"nfl\" checked") && page.contains("NFL &lt;pro&gt;"));
        assert!(page.contains("Pick &lt;one&gt;"));
        let t = town("Kansas City", Units::Celsius, None);
        assert!(t.contains("<li class=\"done\">Sports</li><li class=\"done\">Teams</li><li class=\"now\">Town</li>"));
        assert!(t.contains("value=\"celsius\" checked") && t.contains("href=\"/welcome/fantasy\">Skip"));
    }

    #[test]
    fn teams_step_groups_by_league() {
        let bills = TeamId("espn:nfl:2".into());
        let groups: TeamGroups = vec![
            ("NFL".into(), vec![(bills.clone(), "Buffalo Blizzard".into())]),
            (
                "College Football".into(),
                (0..50).map(|i| (TeamId(format!("espn:ncaaf:{i}")), format!("Team {i}"))).collect(),
            ),
            ("NBA".into(), vec![]),
        ];
        let page = teams(&groups, std::slice::from_ref(&bills));
        assert!(page.contains("value=\"espn:nfl:2\" checked> Buffalo Blizzard"));
        assert!(page.contains("1 picked"));
        assert_eq!(page.matches("team-filter").count(), 1, "only the big league gets a filter");
        assert!(page.contains("Still loading these teams"));
    }
}
