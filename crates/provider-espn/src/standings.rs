//! Pure conversion from ESPN standings JSON
//! (`/apis/v2/sports/<sport>/<league>/standings`) to [`Standings`].

use chrono::{DateTime, Utc};
use marqueet_core::provider::ProviderError;
use marqueet_core::sports::standings::{Standings, StandingsGroup, StandingsRow};
use marqueet_core::sports::{LeagueId, Sport};

use crate::leagues::LeagueDef;
use crate::model::{StandingsEntry, StandingsNode};
use crate::normalize::team_id;

/// Parses a standings response for `league`. Rows come out best first.
pub fn normalize_standings(
    body: &str,
    league: &LeagueDef,
    fetched_at: DateTime<Utc>,
) -> Result<Standings, ProviderError> {
    let root: StandingsNode = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let mut groups = Vec::new();
    collect(&root, league, &mut groups);
    if groups.is_empty() {
        return Err(ProviderError::Parse("no standings in response".into()));
    }
    Ok(Standings { league: LeagueId::new(league.id), sport: league.sport, groups, fetched_at })
}

fn collect(node: &StandingsNode, league: &LeagueDef, out: &mut Vec<StandingsGroup>) {
    if let Some(table) = &node.standings {
        let mut rows: Vec<(StandingsRow, Sort)> =
            table.entries.iter().filter(|e| !e.team.id.is_empty()).map(|e| row(e, league)).collect();
        if !rows.is_empty() {
            rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            out.push(StandingsGroup {
                name: group_name(node, league),
                rows: rows.into_iter().map(|(r, _)| r).collect(),
            });
        }
    }
    for child in &node.children {
        collect(child, league, out);
    }
}

/// "AL East" (ESPN's short name), "Atlantic" (not "Atlantic Division"),
/// or the league name for a table named after the season.
fn group_name(node: &StandingsNode, league: &LeagueDef) -> String {
    if let Some(short) = node.short_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return short.to_owned();
    }
    let name = node.name.trim();
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return league.name.to_owned();
    }
    name.strip_suffix(" Division").unwrap_or(name).to_owned()
}

/// Sort key, ascending = better.
type Sort = (f64, f64, f64);

fn row(e: &StandingsEntry, league: &LeagueDef) -> (StandingsRow, Sort) {
    let stat = |name: &str| e.stats.iter().find(|s| s.name.as_deref() == Some(name));
    let value = |name: &str| stat(name).and_then(|s| s.value);
    let count = |name: &str| value(name).map_or(0, |v| v.max(0.0).round() as u32);
    let display = |name: &str| stat(name).and_then(|s| s.display_value.clone()).filter(|s| !s.is_empty());
    let (wins, losses, ties, ot_losses) = (count("wins"), count("losses"), count("ties"), count("otLosses"));
    let games_played = value("gamesPlayed").map_or(wins + losses + ties + ot_losses, |v| v.round() as u32);
    let points = match league.sport {
        Sport::Hockey | Sport::Soccer => value("points").map(|p| p.round() as i32),
        _ => None,
    };
    let row = StandingsRow {
        team: team_id(league, &e.team.id),
        abbreviation: e.team.abbreviation.clone(),
        wins,
        losses,
        ties,
        ot_losses,
        points,
        games_played,
        win_pct: display("winPercent"),
        games_behind: display("gamesBehind"),
    };
    let sort = match league.sport {
        Sport::Soccer => match value("rank") {
            Some(rank) => (rank, 0.0, 0.0),
            None => (-f64::from(points.unwrap_or(0)), 0.0, 0.0),
        },
        Sport::Hockey => (-f64::from(points.unwrap_or(0)), f64::from(games_played), -f64::from(wins)),
        _ => (-value("winPercent").unwrap_or(0.0), -f64::from(wins), f64::from(losses)),
    };
    (row, sort)
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct TeamsBody {
    sports: Vec<TeamsSport>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct TeamsSport {
    leagues: Vec<TeamsLeague>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct TeamsLeague {
    teams: Vec<TeamsEntry>,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct TeamsEntry {
    team: TeamJson,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TeamJson {
    id: String,
    abbreviation: String,
    display_name: String,
    is_active: Option<bool>,
}

/// Parses `/apis/site/v2/sports/<path>/teams`: active teams, by name.
pub fn normalize_teams(
    body: &str,
    league: &LeagueDef,
) -> Result<Vec<marqueet_core::provider::TeamInfo>, ProviderError> {
    let parsed: TeamsBody = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let mut teams: Vec<marqueet_core::provider::TeamInfo> = parsed
        .sports
        .into_iter()
        .flat_map(|s| s.leagues)
        .flat_map(|l| l.teams)
        .map(|e| e.team)
        .filter(|t| !t.id.is_empty() && t.is_active != Some(false))
        .map(|t| marqueet_core::provider::TeamInfo {
            id: team_id(league, &t.id),
            name: if t.display_name.is_empty() { t.abbreviation.clone() } else { t.display_name },
            abbreviation: t.abbreviation,
        })
        .collect();
    teams.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(teams)
}
