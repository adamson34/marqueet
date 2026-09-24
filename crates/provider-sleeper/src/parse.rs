//! Pure conversion from Sleeper API JSON to the fantasy model. Lenient:
//! every field is optional or defaulted and unknown fields are ignored.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use marqueet_core::fantasy::{FantasyLeagueInfo, FantasyTeam, FantasyTeamInfo, FantasyUser, Matchup, Starter};
use marqueet_core::provider::ProviderError;
use serde::{Deserialize, Serialize};

fn parse<'a, T: Deserialize<'a>>(body: &'a str) -> Result<T, ProviderError> {
    serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct UserJson {
    user_id: String,
    display_name: Option<String>,
    metadata: Option<HashMap<String, serde_json::Value>>,
}

/// `/user/<name>`. Sleeper answers `null` for unknown names.
pub fn user(body: &str) -> Result<Option<FantasyUser>, ProviderError> {
    let u: Option<UserJson> = parse(body)?;
    Ok(u.filter(|u| !u.user_id.is_empty())
        .map(|u| FantasyUser { display_name: u.display_name.unwrap_or_else(|| u.user_id.clone()), id: u.user_id }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct LeagueJson {
    pub league_id: String,
    pub name: String,
    pub season: String,
    pub total_rosters: u32,
    pub roster_positions: Vec<String>,
}

/// `/user/<id>/leagues/nfl/<season>`.
pub fn leagues(body: &str) -> Result<Vec<FantasyLeagueInfo>, ProviderError> {
    let list: Option<Vec<LeagueJson>> = parse(body)?;
    Ok(list
        .unwrap_or_default()
        .into_iter()
        .filter(|l| !l.league_id.is_empty())
        .map(|l| FantasyLeagueInfo { id: l.league_id, name: l.name, season: l.season, teams: l.total_rosters })
        .collect())
}

/// `/league/<id>`.
pub(crate) fn league(body: &str) -> Result<LeagueJson, ProviderError> {
    parse(body)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RosterJson {
    roster_id: u32,
    owner_id: Option<String>,
    settings: HashMap<String, serde_json::Value>,
}

impl RosterJson {
    fn record(&self) -> String {
        let n = |k: &str| self.settings.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
        match n("ties") {
            0 => format!("{}-{}", n("wins"), n("losses")),
            t => format!("{}-{}-{t}", n("wins"), n("losses")),
        }
    }
}

pub(crate) fn rosters(body: &str) -> Result<Vec<RosterJson>, ProviderError> {
    Ok(parse::<Option<Vec<RosterJson>>>(body)?.unwrap_or_default())
}

pub(crate) fn users(body: &str) -> Result<Vec<UserJson>, ProviderError> {
    Ok(parse::<Option<Vec<UserJson>>>(body)?.unwrap_or_default())
}

/// Team name for a roster: the manager's team name, else their display
/// name, else "Team N".
fn team_name(roster: &RosterJson, users: &[UserJson]) -> String {
    let user = roster.owner_id.as_deref().and_then(|id| users.iter().find(|u| u.user_id == id));
    user.and_then(|u| u.metadata.as_ref()?.get("team_name")?.as_str().map(str::trim).map(str::to_owned))
        .filter(|n| !n.is_empty())
        .or_else(|| user.and_then(|u| u.display_name.clone()))
        .unwrap_or_else(|| format!("Team {}", roster.roster_id))
}

/// Teams in a league from `/league/<id>/rosters` and `/league/<id>/users`.
pub fn teams(rosters_body: &str, users_body: &str) -> Result<Vec<FantasyTeamInfo>, ProviderError> {
    let (rosters, users) = (rosters(rosters_body)?, users(users_body)?);
    let mut out: Vec<FantasyTeamInfo> = rosters
        .iter()
        .map(|r| FantasyTeamInfo { roster_id: r.roster_id, name: team_name(r, &users), owner_id: r.owner_id.clone() })
        .collect();
    out.sort_by_key(|t| t.roster_id);
    Ok(out)
}

/// What we keep of each NFL player from the (large) `/players/nfl` file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerInfo {
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub position: Option<String>,
    pub team: Option<String>,
    #[serde(deserialize_with = "id_string")]
    pub espn_id: Option<String>,
}

/// ESPN ids arrive as numbers or strings.
fn id_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<serde_json::Value>::deserialize(d)? {
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s),
        _ => None,
    })
}

/// `/players/nfl`: player id → the fields we use.
pub fn players(body: &str) -> Result<HashMap<String, PlayerInfo>, ProviderError> {
    parse(body)
}

/// "J. Allen"; a defense is its team ("PHI").
fn short_name(id: &str, p: Option<&PlayerInfo>) -> String {
    let Some(p) = p else { return id.to_owned() };
    if p.position.as_deref() == Some("DEF") {
        return p.team.clone().unwrap_or_else(|| id.to_owned());
    }
    match (p.first_name.as_deref(), p.last_name.as_deref()) {
        (Some(f), Some(l)) if !f.is_empty() => format!("{}. {l}", f.chars().next().unwrap_or(' ')),
        (_, Some(l)) => l.to_owned(),
        _ => id.to_owned(),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct MatchupJson {
    roster_id: u32,
    matchup_id: Option<u32>,
    points: Option<f64>,
    starters: Vec<String>,
    starters_points: Vec<Option<f64>>,
}

pub(crate) fn matchups(body: &str) -> Result<Vec<MatchupJson>, ProviderError> {
    Ok(parse::<Option<Vec<MatchupJson>>>(body)?.unwrap_or_default())
}

/// Current NFL week and season from `/state/nfl`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct NflState {
    pub week: u32,
    pub display_week: u32,
    pub season: String,
    pub league_season: String,
    pub season_type: String,
}

pub fn state(body: &str) -> Result<NflState, ProviderError> {
    parse(body)
}

/// Everything [`build_matchup`] needs, already parsed.
#[derive(Debug)]
pub(crate) struct LeagueData<'a> {
    pub league: &'a LeagueJson,
    pub users: &'a [UserJson],
    pub rosters: &'a [RosterJson],
    pub matchups: &'a [MatchupJson],
    pub players: &'a HashMap<String, PlayerInfo>,
}

fn team(d: &LeagueData<'_>, m: &MatchupJson) -> FantasyTeam {
    let roster = d.rosters.iter().find(|r| r.roster_id == m.roster_id);
    let slots = d.league.roster_positions.iter().filter(|p| !matches!(p.as_str(), "BN" | "IR" | "TAXI"));
    let starters = m
        .starters
        .iter()
        .zip(slots.map(String::as_str).chain(std::iter::repeat("")))
        .enumerate()
        .map(|(i, (id, slot))| {
            let info = d.players.get(id);
            let empty = id == "0" || id.is_empty();
            Starter {
                slot: slot.to_owned(),
                player_id: id.clone(),
                name: if empty { "EMPTY".into() } else { short_name(id, info) },
                position: info.and_then(|p| p.position.clone()).unwrap_or_default(),
                team: info.and_then(|p| p.team.clone()),
                espn_id: info.and_then(|p| p.espn_id.clone()),
                points: m.starters_points.get(i).copied().flatten().unwrap_or(0.0) as f32,
            }
        })
        .collect();
    FantasyTeam {
        roster_id: m.roster_id,
        name: roster.map_or_else(|| format!("Team {}", m.roster_id), |r| team_name(r, d.users)),
        record: roster.map(RosterJson::record).unwrap_or_default(),
        points: m.points.unwrap_or(0.0) as f32,
        starters,
    }
}

/// `roster_id`'s matchup this week, with its opponent (same `matchup_id`).
pub(crate) fn build_matchup(
    d: &LeagueData<'_>,
    roster_id: u32,
    week: u32,
    now: DateTime<Utc>,
) -> Result<Matchup, ProviderError> {
    let mine = d
        .matchups
        .iter()
        .find(|m| m.roster_id == roster_id)
        .ok_or_else(|| ProviderError::NotFound(format!("a week {week} matchup (has the league drafted?)")))?;
    let opponent =
        mine.matchup_id.and_then(|id| d.matchups.iter().find(|m| m.matchup_id == Some(id) && m.roster_id != roster_id));
    Ok(Matchup {
        league_id: d.league.league_id.clone(),
        league: d.league.name.clone(),
        week,
        me: team(d, mine),
        opponent: opponent.map(|o| team(d, o)),
        fetched_at: now,
    })
}

/// Builds a matchup straight from response bodies (tests, tools).
#[allow(clippy::too_many_arguments)]
pub fn matchup_from_bodies(
    league_body: &str,
    users_body: &str,
    rosters_body: &str,
    matchups_body: &str,
    players: &HashMap<String, PlayerInfo>,
    roster_id: u32,
    week: u32,
    now: DateTime<Utc>,
) -> Result<Matchup, ProviderError> {
    let (league, users, rosters, matchups) =
        (league(league_body)?, users(users_body)?, rosters(rosters_body)?, matchups(matchups_body)?);
    let data = LeagueData { league: &league, users: &users, rosters: &rosters, matchups: &matchups, players };
    build_matchup(&data, roster_id, week, now)
}
