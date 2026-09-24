//! Fantasy football, normalized: a team's matchup this week with each
//! starter's live points. Providers (Sleeper first) fill these in. Pure.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::sports::Athlete;
use crate::ticker::{Align, Part, Span, TickerSegment, Tint};

/// A fantasy platform account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FantasyUser {
    pub id: String,
    pub display_name: String,
}

/// A league the user is in, for picking one on the admin page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FantasyLeagueInfo {
    pub id: String,
    pub name: String,
    pub season: String,
    pub teams: u32,
}

/// A team in a league, for picking one on the admin page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FantasyTeamInfo {
    pub roster_id: u32,
    /// Team name, or the manager's name when there's none.
    pub name: String,
    pub owner_id: Option<String>,
}

/// A starter and what they've scored this week.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Starter {
    /// Lineup slot: "QB", "RB", "FLEX", "K", "DEF".
    pub slot: String,
    pub player_id: String,
    /// "R. Castellano", or the team for a defense: "PHI".
    pub name: String,
    pub position: String,
    /// NFL team abbreviation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// ESPN athlete id, to match plays in live games.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub espn_id: Option<String>,
    pub points: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FantasyTeam {
    pub roster_id: u32,
    pub name: String,
    /// "7-7" or "7-6-1".
    pub record: String,
    pub points: f32,
    pub starters: Vec<Starter>,
}

/// One team's matchup this week.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Matchup {
    pub league_id: String,
    pub league: String,
    pub week: u32,
    pub me: FantasyTeam,
    /// `None` on a bye (or in leagues without head-to-head matchups).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opponent: Option<FantasyTeam>,
    pub fetched_at: DateTime<Utc>,
}

impl Matchup {
    /// The starter with this ESPN athlete id on either side, and whether
    /// they're on my team.
    pub fn starter_by_espn_id(&self, espn_id: &str) -> Option<(&Starter, bool)> {
        let mine = self.me.starters.iter().find(|s| s.espn_id.as_deref() == Some(espn_id)).map(|s| (s, true));
        mine.or_else(|| {
            self.opponent.as_ref()?.starters.iter().find(|s| s.espn_id.as_deref() == Some(espn_id)).map(|s| (s, false))
        })
    }
}

/// Points as fantasy apps show them: "88.42" → "88.4".
pub fn points(p: f32) -> String {
    format!("{p:.1}")
}

/// A takeover note when a play involves followed fantasy starters:
/// ("YOUR STARTER", "R. CASTELLANO  24.1 PTS"), or "THEIR STARTER" for the
/// opponent's. The bool is true when the starter is mine. My starters win
/// when both sides are involved (a pass from my QB to their WR).
pub fn takeover_note(matchups: &[Matchup], athletes: &[Athlete]) -> Option<(String, String, bool)> {
    let found: Vec<(&Starter, bool)> =
        athletes.iter().flat_map(|a| matchups.iter().filter_map(move |m| m.starter_by_espn_id(&a.id))).collect();
    let mine: Vec<&Starter> = found.iter().filter(|(_, m)| *m).map(|(s, _)| *s).collect();
    let (label, list, is_mine) = if mine.is_empty() {
        ("THEIR STARTER", found.iter().map(|(s, _)| *s).collect::<Vec<_>>(), false)
    } else {
        ("YOUR STARTER", mine, true)
    };
    let first = list.first()?;
    let value = if list.len() > 1 {
        let names: Vec<String> = list.iter().take(2).map(|s| s.name.to_uppercase()).collect();
        names.join(" + ")
    } else {
        format!("{}  {} PTS", first.name.to_uppercase(), points(first.points))
    };
    let label = if list.len() > 1 { label.replace("STARTER", "STARTERS") } else { label.to_owned() };
    Some((label, value, is_mine))
}

/// A team name the LED font can draw: printable ASCII, uppercased, at most
/// `max` characters (fantasy names are full of emoji).
pub fn led_name(name: &str, max: usize) -> String {
    let cleaned: String = name.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').collect();
    let words = cleaned.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase();
    let mut out: String = words.chars().take(max).collect();
    if out.len() < words.len() {
        out = out.trim_end().to_owned();
    }
    if out.is_empty() { "TEAM".into() } else { out }
}

/// Ticker segment id for a followed team.
pub fn segment_id(league_id: &str, roster_id: u32) -> String {
    format!("fantasy:{league_id}:{roster_id}")
}

/// The matchup as a ticker block: team names stacked over the opponent's,
/// scores with the leader bright, then the week.
pub fn ticker_segment(m: &Matchup) -> TickerSegment {
    let me = led_name(&m.me.name, 14);
    let week = Span::dim(format!("WK {}", m.week));
    let parts = match &m.opponent {
        Some(o) => {
            let leading = m.me.points >= o.points;
            let tint = |lead: bool| if lead { Tint::Primary } else { Tint::Dim };
            vec![
                Part::stack(vec![Span::primary(me)], vec![Span::primary(led_name(&o.name, 14))], Align::Left),
                Part::gap(4),
                Part::stack(
                    vec![Span::new(points(m.me.points), tint(leading))],
                    vec![Span::new(points(o.points), tint(!leading || m.me.points == o.points))],
                    Align::Right,
                ),
                Part::gap(5),
                Part::stack(vec![Span::new("FANTASY", Tint::Accent)], vec![week], Align::Left),
            ]
        }
        None => vec![
            Part::stack(vec![Span::primary(me)], vec![Span::dim("BYE")], Align::Left),
            Part::gap(4),
            Part::text(vec![Span::primary(points(m.me.points))]),
            Part::gap(5),
            Part::stack(vec![Span::new("FANTASY", Tint::Accent)], vec![week], Align::Left),
        ],
    };
    TickerSegment { id: segment_id(&m.league_id, m.me.roster_id), parts }
}

/// Demo matchup (mock display and tests).
pub fn mock_matchup(now: DateTime<Utc>) -> Matchup {
    let s = |slot: &str, name: &str, pos: &str, team: &str, espn: Option<&str>, pts: f32| Starter {
        slot: slot.into(),
        player_id: name.into(),
        name: name.into(),
        position: pos.into(),
        team: Some(team.into()),
        espn_id: espn.map(Into::into),
        points: pts,
    };
    Matchup {
        league_id: "1000000000000000001".into(),
        league: "Office League".into(),
        week: 3,
        me: FantasyTeam {
            roster_id: 2,
            name: "Hail Mary Bros 🏈".into(),
            record: "2-0".into(),
            points: 98.4,
            starters: vec![
                s("QB", "R. Castellano", "QB", "BUF", Some("9000001"), 24.1),
                s("RB", "D. Okafor", "RB", "ATL", None, 14.2),
                s("RB", "T. Lindqvist", "RB", "DET", None, 11.8),
                s("WR", "M. Duval", "WR", "MIN", Some("9000002"), 17.3),
                s("WR", "K. Abernathy", "WR", "DET", None, 9.6),
                s("TE", "G. Pruitt", "TE", "DET", None, 6.1),
                s("FLEX", "L. Mensah", "RB", "MIA", None, 8.4),
                s("K", "E. Salgado", "K", "DAL", None, 7.0),
                s("DEF", "PHI", "DEF", "PHI", None, 0.0),
            ],
        },
        opponent: Some(FantasyTeam {
            roster_id: 1,
            name: "Punt Intended".into(),
            record: "1-1".into(),
            points: 91.7,
            starters: vec![
                s("QB", "C. Whitlock", "QB", "KC", Some("9000003"), 21.4),
                s("RB", "A. Brandt", "RB", "PHI", None, 18.9),
                s("RB", "J. Moreau", "RB", "LAR", None, 9.2),
                s("WR", "D. Kowalski", "WR", "DAL", None, 12.5),
                s("WR", "S. Iwu", "WR", "LAR", None, 10.1),
                s("TE", "B. Hollis", "TE", "KC", None, 7.7),
                s("FLEX", "R. Tanaka", "WR", "NYS", None, 5.9),
                s("K", "O. Lindgren", "K", "KC", None, 6.0),
                s("DEF", "BAL", "DEF", "BAL", None, 0.0),
            ],
        }),
        fetched_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::ticker::segment_text;

    #[test]
    fn takeover_notes_for_fantasy_starters() {
        let m = mock_matchup(Utc::now());
        let a = |id: &str| Athlete { id: id.into(), name: String::new() };
        assert_eq!(
            takeover_note(std::slice::from_ref(&m), &[a("9000001")]),
            Some(("YOUR STARTER".into(), "R. CASTELLANO  24.1 PTS".into(), true))
        );
        assert_eq!(
            takeover_note(std::slice::from_ref(&m), &[a("9000003")]),
            Some(("THEIR STARTER".into(), "C. WHITLOCK  21.4 PTS".into(), false))
        );
        let both = takeover_note(std::slice::from_ref(&m), &[a("9000003"), a("9000001"), a("9000002")]).unwrap();
        assert_eq!(both, ("YOUR STARTERS".into(), "R. CASTELLANO + M. DUVAL".into(), true), "mine win, two named");
        assert_eq!(takeover_note(&[m], &[a("1")]), None);
        assert_eq!(takeover_note(&[], &[a("9000001")]), None);
    }

    #[test]
    fn names_the_led_font_can_draw() {
        assert_eq!(led_name("Hail Mary Bros 🏈", 14), "HAIL MARY BROS");
        assert_eq!(led_name("  the   Real  Slim Shady  ", 14), "THE REAL SLIM");
        assert_eq!(led_name("🔥🔥🔥", 14), "TEAM");
    }

    #[test]
    fn matchup_on_the_ticker() {
        let mut m = mock_matchup(Utc::now());
        let seg = ticker_segment(&m);
        assert_eq!(seg.id, "fantasy:1000000000000000001:2");
        assert_eq!(segment_text(&seg), "HAIL MARY BROS/PUNT INTENDED 98.4/91.7 FANTASY/WK 3");
        let Part::Stack { top, bottom, .. } = &seg.parts[2] else { panic!() };
        assert_eq!((top[0].tint, bottom[0].tint), (Tint::Primary, Tint::Dim), "leader bright");
        m.opponent = None;
        assert_eq!(segment_text(&ticker_segment(&m)), "HAIL MARY BROS/BYE 98.4 FANTASY/WK 3");
    }
}
