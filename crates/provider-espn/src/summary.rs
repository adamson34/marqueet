//! ESPN's per-game summary (`.../summary?event=<id>`) into a
//! [`GameSummary`]: team stats, leaders, scoring plays, win probability.
//! Lenient: anything missing or oddly shaped is skipped, never an error.

use marqueet_core::provider::ProviderError;
use marqueet_core::sports::Game;
use marqueet_core::sports::Sport;
use marqueet_core::sports::summary::{AtBat, Call, GameSummary, LeaderRow, Pitch, PlayerLine, ScoringPlay, TeamStat};
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

/// ESPN's pitch coordinates: the strike zone's centre and half-size, found
/// from where called strikes and balls land (x across, y down).
const ZONE_CENTRE: (f64, f64) = (116.0, 169.0);
const ZONE_HALF: (f64, f64) = (28.0, 25.0);

/// A player's name and line from the box score: a pitcher's "5.2 IP, 4 H,
/// 1 ER, 7 K, 1 BB, 84 P" or a batter's "1-2, HR, 2 RBI".
fn player_line(doc: &Value, id: &str, pitching: bool) -> Option<PlayerLine> {
    let kind = if pitching { "pitching" } else { "batting" };
    for team in doc.pointer("/boxscore/players").and_then(Value::as_array).into_iter().flatten() {
        for group in team.get("statistics").and_then(Value::as_array).into_iter().flatten() {
            if group.get("type").and_then(Value::as_str) != Some(kind) {
                continue;
            }
            let names: Vec<String> =
                group.get("names").and_then(Value::as_array).into_iter().flatten().filter_map(str_of).collect();
            for a in group.get("athletes").and_then(Value::as_array).into_iter().flatten() {
                if a.pointer("/athlete/id").and_then(str_of).as_deref() != Some(id) {
                    continue;
                }
                let name =
                    a.pointer("/athlete/shortName").or_else(|| a.pointer("/athlete/displayName")).and_then(str_of)?;
                let stats: Vec<String> =
                    a.get("stats").and_then(Value::as_array).into_iter().flatten().filter_map(str_of).collect();
                let stat = |key: &str| names.iter().position(|n| n == key).and_then(|i| stats.get(i)).cloned();
                let line = if pitching {
                    [("IP", "IP"), ("H", "H"), ("ER", "ER"), ("K", "K"), ("BB", "BB"), ("PC", "P")]
                        .iter()
                        .filter_map(|(key, unit)| stat(key).map(|v| format!("{v} {unit}")))
                        .collect::<Vec<_>>()
                        .join(", ")
                } else {
                    let mut parts: Vec<String> = stat("H-AB").into_iter().collect();
                    for (key, unit) in [("HR", "HR"), ("RBI", "RBI"), ("BB", "BB")] {
                        match stat(key).and_then(|v| v.parse::<u32>().ok()) {
                            Some(1) => parts.push(unit.to_owned()),
                            Some(n) if n > 1 => parts.push(format!("{n} {unit}")),
                            _ => {}
                        }
                    }
                    parts.join(", ")
                };
                return Some(PlayerLine { name, line });
            }
        }
    }
    None
}

/// The at-bat in progress (baseball): the count and outs from `situation`,
/// the pitcher and batter from it and the box score, the runners from the
/// latest play, and this at-bat's pitches. `None` between at-bats.
fn at_bat(doc: &Value) -> Option<AtBat> {
    let situation = doc.get("situation")?;
    let id_of = |key: &str| situation.pointer(&format!("/{key}/playerId")).and_then(str_of);
    let batter_id = id_of("batter")?;
    let plays = doc.get("plays").and_then(Value::as_array)?;
    let current = plays.iter().rev().find_map(|p| p.get("atBatId").and_then(str_of))?;
    let count = |key: &str| situation.get(key).and_then(Value::as_u64).unwrap_or(0).min(9) as u8;
    let last = plays.last()?;
    let on = |base: &str| last.get(base).is_some_and(|v| !v.is_null());
    let in_current = |p: &&Value| p.get("atBatId").and_then(str_of).as_deref() == Some(current.as_str());
    // Right after an at-bat ends the next batter is already named; the
    // pitches so far belong to the one before.
    let batter_of = |p: &&Value| {
        p.get("participants").and_then(Value::as_array).into_iter().flatten().find_map(|x| {
            (x.get("type").and_then(Value::as_str) == Some("batter"))
                .then(|| x.pointer("/athlete/id").and_then(str_of))?
        })
    };
    let same_batter = plays.iter().filter(in_current).find_map(|p| batter_of(&p)).is_none_or(|b| b == batter_id);
    let pitches = plays
        .iter()
        .filter(in_current)
        .filter(|_| same_batter)
        .filter(|p| p.get("summaryType").and_then(Value::as_str) == Some("P"))
        .enumerate()
        .map(|(i, p)| {
            let kind = p.pointer("/type/type").and_then(Value::as_str).unwrap_or("");
            let call = if kind.starts_with("ball") {
                Call::Ball
            } else if kind.starts_with("strike") || kind.starts_with("foul") {
                Call::Strike
            } else {
                Call::InPlay
            };
            let at = p.get("pitchCoordinate").and_then(|c| {
                let (x, y) = (c.get("x")?.as_f64()?, c.get("y")?.as_f64()?);
                let nx = (x - ZONE_CENTRE.0) / ZONE_HALF.0;
                let ny = (y - ZONE_CENTRE.1) / ZONE_HALF.1;
                Some((nx.clamp(-3.0, 3.0) as f32, ny.clamp(-3.0, 3.0) as f32))
            });
            let pitch_type = p.pointer("/pitchType/text").and_then(str_of).unwrap_or_default();
            let mph = p.get("pitchVelocity").and_then(Value::as_u64);
            let kind = match mph {
                Some(v) => format!("{pitch_type} {v}"),
                None => pitch_type,
            };
            Pitch {
                number: p.get("atBatPitchNumber").and_then(Value::as_u64).map_or(i + 1, |n| n as usize).min(99) as u8,
                at,
                result: p.pointer("/type/text").and_then(str_of).unwrap_or_default().to_uppercase(),
                kind: kind.trim().to_uppercase(),
                call,
            }
        })
        .collect();
    Some(AtBat {
        pitcher: id_of("pitcher").and_then(|id| player_line(doc, &id, true)),
        batter: player_line(doc, &batter_id, false),
        balls: count("balls"),
        strikes: count("strikes"),
        outs: count("outs"),
        bases: [on("onFirst"), on("onSecond"), on("onThird")],
        pitches,
    })
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
                text: crate::normalize::one_line(&p.get("text").and_then(str_of)?),
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

    let at_bat = (game.sport == Sport::Baseball).then(|| at_bat(&doc)).flatten();
    Ok(GameSummary { team_stats, leaders, scoring, home_win, at_bat })
}
