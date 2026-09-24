//! ESPN's per-game summary (`.../summary?event=<id>`) into a
//! [`GameSummary`]: team stats, leaders, scoring plays, win probability.
//! Lenient: anything missing or oddly shaped is skipped, never an error.

use marqueet_core::provider::ProviderError;
use marqueet_core::sports::Game;
use marqueet_core::sports::summary::{GameSummary, LeaderRow, ScoringPlay, TeamStat};
use serde_json::Value;

/// ESPN's own id for a team from our id (`espn:nfl:12` → `12`).
fn espn_id(game_team: &str) -> &str {
    game_team.rsplit(':').next().unwrap_or(game_team)
}

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Which side a summary team block is: `Some(true)` home, `Some(false)` away.
fn side(block: &Value, away: &str, home: &str) -> Option<bool> {
    let id = block.pointer("/team/id").and_then(str_of)?;
    if id == home {
        Some(true)
    } else if id == away {
        Some(false)
    } else {
        None
    }
}

/// A team's stats as (key, label, value), flattening grouped stats
/// (baseball's batting/pitching/fielding) into one list.
fn stats(block: &Value) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for s in block.get("statistics").and_then(Value::as_array).into_iter().flatten() {
        if let Some(group) = s.get("stats").and_then(Value::as_array) {
            for g in group {
                push_stat(&mut out, g);
            }
        } else {
            push_stat(&mut out, s);
        }
    }
    out
}

fn push_stat(out: &mut Vec<(String, String, String)>, s: &Value) {
    let (Some(name), Some(value)) = (s.get("name").and_then(str_of), s.get("displayValue").and_then(str_of)) else {
        return;
    };
    // Keep the first of a repeated key (baseball reuses names across groups).
    if out.iter().any(|(n, ..)| *n == name) {
        return;
    }
    let label = s
        .get("label")
        .or_else(|| s.get("displayName"))
        .or_else(|| s.get("shortDisplayName"))
        .and_then(str_of)
        .unwrap_or_else(|| name.clone());
    out.push((name, label, value));
}

/// A leader as "R. Castellano 212 YDS".
fn leader_text(l: &Value) -> Option<String> {
    let name = l.pointer("/athlete/shortName").or_else(|| l.pointer("/athlete/displayName")).and_then(str_of)?;
    let value = l.get("displayValue").and_then(str_of).unwrap_or_default();
    // Football's passing line reads "10/17, 86 YDS, 2 INT"; keep it short.
    let value = value.split(", ").filter(|p| !p.contains('/')).collect::<Vec<_>>().join(" ");
    Some(format!("{name} {value}").trim().to_owned())
}

/// Parses a summary for `game` (which says who's home and away).
pub fn normalize_summary(body: &str, game: &Game) -> Result<GameSummary, ProviderError> {
    let doc: Value = serde_json::from_str(body).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let (away, home) = (espn_id(&game.away.team.id.0), espn_id(&game.home.team.id.0));

    let mut away_stats = Vec::new();
    let mut home_stats = Vec::new();
    for block in doc.pointer("/boxscore/teams").and_then(Value::as_array).into_iter().flatten() {
        match side(block, away, home) {
            Some(true) => home_stats = stats(block),
            Some(false) => away_stats = stats(block),
            None => {}
        }
    }
    let team_stats = away_stats
        .into_iter()
        .filter_map(|(name, label, a)| {
            let h = home_stats.iter().find(|(n, ..)| *n == name)?.2.clone();
            Some(TeamStat { name, label, away: a, home: h })
        })
        .collect();

    // Leaders: categories per team; pair them up by category, away first.
    let mut leaders: Vec<LeaderRow> = Vec::new();
    for block in doc.get("leaders").and_then(Value::as_array).into_iter().flatten() {
        let Some(is_home) = side(block, away, home) else { continue };
        for cat in block.get("leaders").and_then(Value::as_array).into_iter().flatten() {
            let Some(category) = cat.get("displayName").or_else(|| cat.get("name")).and_then(str_of) else { continue };
            let text = cat.get("leaders").and_then(Value::as_array).and_then(|l| l.first()).and_then(leader_text);
            let i = match leaders.iter().position(|r| r.category == category) {
                Some(i) => i,
                None => {
                    leaders.push(LeaderRow { category, away: None, home: None });
                    leaders.len() - 1
                }
            };
            if is_home {
                leaders[i].home = text;
            } else {
                leaders[i].away = text;
            }
        }
    }

    let scoring = doc
        .get("scoringPlays")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(ScoringPlay {
                period: p.pointer("/period/number").and_then(Value::as_u64).unwrap_or(0).min(99) as u8,
                clock: p.pointer("/clock/displayValue").and_then(str_of).unwrap_or_default(),
                team: p.pointer("/team/abbreviation").and_then(str_of),
                kind: p.pointer("/type/abbreviation").and_then(str_of),
                text: p.get("text").and_then(str_of)?,
                away_score: p.get("awayScore").and_then(Value::as_u64).unwrap_or(0).min(u64::from(u16::MAX)) as u16,
                home_score: p.get("homeScore").and_then(Value::as_u64).unwrap_or(0).min(u64::from(u16::MAX)) as u16,
            })
        })
        .collect();

    let home_win = doc
        .get("winprobability")
        .and_then(Value::as_array)
        .and_then(|w| w.last())
        .and_then(|w| w.get("homeWinPercentage"))
        .and_then(Value::as_f64)
        .map(|p| (p * 100.0).round().clamp(0.0, 100.0) as u8);

    Ok(GameSummary { team_stats, leaders, scoring, home_win })
}
