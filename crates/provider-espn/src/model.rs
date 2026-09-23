//! Lenient serde models for ESPN's scoreboard JSON.
//!
//! The API is unofficial, so every field is optional or defaulted and
//! numbers that sometimes arrive as strings are accepted either way. Unknown
//! fields are ignored.

use serde::{Deserialize, Deserializer};

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub date: String,
    pub competitions: Vec<Competition>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Competition {
    pub status: Status,
    pub competitors: Vec<Competitor>,
    pub situation: Option<Situation>,
    pub broadcasts: Vec<Broadcast>,
    pub venue: Option<Venue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Status {
    pub display_clock: Option<String>,
    #[serde(deserialize_with = "num")]
    pub period: Option<i64>,
    #[serde(rename = "type")]
    pub kind: StatusType,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StatusType {
    pub name: String,
    /// `pre`, `in` or `post`.
    pub state: String,
    pub completed: bool,
    pub detail: String,
    pub short_detail: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Competitor {
    pub home_away: String,
    #[serde(deserialize_with = "num")]
    pub score: Option<i64>,
    pub winner: Option<bool>,
    pub team: Team,
    pub records: Vec<Record>,
    pub curated_rank: Option<Rank>,
    pub linescores: Vec<Linescore>,
    #[serde(deserialize_with = "num")]
    pub hits: Option<i64>,
    #[serde(deserialize_with = "num")]
    pub errors: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Team {
    pub id: String,
    pub abbreviation: String,
    pub display_name: String,
    pub short_display_name: String,
    pub color: Option<String>,
    pub alternate_color: Option<String>,
    pub logo: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Record {
    pub summary: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Rank {
    #[serde(deserialize_with = "num")]
    pub current: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Linescore {
    #[serde(deserialize_with = "num")]
    pub value: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Situation {
    #[serde(deserialize_with = "num")]
    pub down: Option<i64>,
    #[serde(deserialize_with = "num")]
    pub distance: Option<i64>,
    pub is_red_zone: bool,
    /// Team id with the ball.
    pub possession: Option<String>,
    pub down_distance_text: Option<String>,
    #[serde(deserialize_with = "num")]
    pub balls: Option<i64>,
    #[serde(deserialize_with = "num")]
    pub strikes: Option<i64>,
    #[serde(deserialize_with = "num")]
    pub outs: Option<i64>,
    pub on_first: bool,
    pub on_second: bool,
    pub on_third: bool,
    pub last_play: Option<LastPlay>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LastPlay {
    pub id: String,
    pub text: String,
    #[serde(rename = "type")]
    pub kind: Option<PlayType>,
    pub team: Option<IdRef>,
    #[serde(deserialize_with = "num")]
    pub score_value: Option<i64>,
    pub athletes_involved: Vec<Athlete>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct PlayType {
    pub text: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct IdRef {
    pub id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Athlete {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Broadcast {
    pub market: Option<String>,
    pub names: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Venue {
    pub full_name: Option<String>,
}

/// A node of the standings tree: the league, a conference, a division.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StandingsNode {
    pub name: String,
    pub short_name: Option<String>,
    pub children: Vec<StandingsNode>,
    pub standings: Option<StandingsTable>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StandingsTable {
    pub entries: Vec<StandingsEntry>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StandingsEntry {
    pub team: StandingsTeam,
    pub stats: Vec<Stat>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StandingsTeam {
    pub id: String,
    pub abbreviation: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Stat {
    pub name: Option<String>,
    #[serde(deserialize_with = "float")]
    pub value: Option<f64>,
    pub display_value: Option<String>,
}

/// Accepts a number, a numeric string, or null.
fn float<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum N {
        Num(f64),
        Str(String),
    }
    Ok(match Option::<N>::deserialize(d)? {
        Some(N::Num(f)) => Some(f),
        Some(N::Str(s)) => s.trim().parse::<f64>().ok(),
        None => None,
    }
    .filter(|f| f.is_finite()))
}

/// Accepts a number, a numeric string, or null.
fn num<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum N {
        Int(i64),
        Float(f64),
        Str(String),
    }
    Ok(match Option::<N>::deserialize(d)? {
        Some(N::Int(i)) => Some(i),
        Some(N::Float(f)) if f.is_finite() => Some(f.round() as i64),
        Some(N::Str(s)) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite()).map(|f| f.round() as i64),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_accept_strings_floats_and_null() {
        let c: Competitor = serde_json::from_str(r#"{"score":"21","hits":5.0,"errors":null}"#).unwrap();
        assert_eq!((c.score, c.hits, c.errors), (Some(21), Some(5), None));
        let c: Competitor = serde_json::from_str(r#"{"score":7}"#).unwrap();
        assert_eq!(c.score, Some(7));
        let c: Competitor = serde_json::from_str(r#"{"score":""}"#).unwrap();
        assert_eq!(c.score, None);
    }

    #[test]
    fn unknown_fields_and_missing_fields_are_fine() {
        let e: Event = serde_json::from_str(r#"{"id":"1","whatever":{"x":1}}"#).unwrap();
        assert_eq!(e.id, "1");
        assert!(e.competitions.is_empty());
    }
}
