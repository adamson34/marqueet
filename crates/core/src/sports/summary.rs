//! A game's details beyond the scoreboard: team stats, leaders, scoring
//! plays and win probability, for the spotlighted game (fetched only for
//! that game). Pure: providers fill it in; [`panels`] turns it into what the
//! spotlight shows.

use serde::{Deserialize, Serialize};

use super::{Game, GameStatus, Sport};
use crate::ticker::{Align, Part, Span, TickerSegment, Tint};
use crate::widgets::StatPanel;

/// One team stat for both sides ("Total Yards", "340", "287").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamStat {
    /// The provider's stat key ("totalYards"), for picking.
    pub name: String,
    pub label: String,
    pub away: String,
    pub home: String,
}

/// One leader category, both teams ("Passing Yards", "R. Castellano 212 YDS").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaderRow {
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub away: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoringPlay {
    pub period: u8,
    /// "2:58"
    pub clock: String,
    /// Scoring team's abbreviation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// "TD", "FG"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    pub text: String,
    pub away_score: u16,
    pub home_score: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameSummary {
    pub team_stats: Vec<TeamStat>,
    pub leaders: Vec<LeaderRow>,
    /// Oldest first.
    pub scoring: Vec<ScoringPlay>,
    /// The home team's chance of winning, 0 to 100, when the provider has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_win: Option<u8>,
    /// Baseball, while an at-bat is on: the pitcher, batter, count and
    /// pitches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_bat: Option<AtBat>,
    /// Football: the drive in progress (or the one that just ended).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive: Option<Drive>,
}

/// A football drive, like a broadcast's drive tracker. Field positions are
/// yards from the offense's own goal line (0) to the end zone (100).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drive {
    /// The offense's abbreviation.
    pub team: String,
    /// True when the offense is the home team (for its colors).
    pub home: bool,
    pub plays: u16,
    pub yards: i32,
    /// "3:21"
    pub time: String,
    /// Where the drive started.
    pub start: u8,
    /// The ball now.
    pub ball: u8,
    /// Where a first down is, when there's a down to play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_down: Option<u8>,
    /// "2nd & 6 at ATL 14"; empty when the drive is over.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub down: String,
    /// How it ended ("Touchdown", "Punt"), once it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// The latest plays, newest first.
    pub recent: Vec<DrivePlay>,
}

/// One play of a drive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrivePlay {
    /// Yards gained (negative for a loss).
    pub yards: i32,
    /// "Pass", "Rush", "Penalty"
    pub kind: String,
    pub text: String,
}

/// The at-bat in progress, like a broadcast's pitch tracker.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AtBat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitcher: Option<PlayerLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batter: Option<PlayerLine>,
    pub balls: u8,
    pub strikes: u8,
    pub outs: u8,
    /// Runners on first, second, third.
    pub bases: [bool; 3],
    /// This at-bat's pitches, first first.
    pub pitches: Vec<Pitch>,
}

/// A player and their line in this game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerLine {
    /// "C. Holmes"
    pub name: String,
    /// Pitcher: "4.1 IP, 5 H, 2 ER, 3 K, 3 BB, 78 P". Batter: "1-2, BB".
    pub line: String,
}

/// One pitch of an at-bat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pitch {
    /// Its number in the at-bat, from 1.
    pub number: u8,
    /// Where it crossed the plate, in strike-zone units from the umpire's
    /// view: the zone spans -1 to 1 both ways (x right, y down); `None`
    /// when the provider didn't place it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<(f32, f32)>,
    /// "STRIKE SWINGING", "BALL", "FOUL BALL", "SINGLE".
    pub result: String,
    /// "CUTTER 83"
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    pub call: Call,
}

/// How a pitch counts, for its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Call {
    Ball,
    Strike,
    InPlay,
}

/// The team stats worth a line for each sport, in order (provider keys; the
/// first that exist are shown).
fn key_stats(sport: Sport) -> &'static [&'static str] {
    match sport {
        Sport::Football => &[
            "totalYards",
            "netPassingYards",
            "rushingYards",
            "turnovers",
            "thirdDownEff",
            "firstDowns",
            "totalPenaltiesYards",
            "possessionTime",
        ],
        Sport::Baseball => &["hits", "homeRuns", "RBIs", "walks", "strikeouts", "errors", "stolenBases"],
        Sport::Basketball => &[
            "fieldGoalsMade-fieldGoalsAttempted",
            "threePointFieldGoalsMade-threePointFieldGoalsAttempted",
            "freeThrowsMade-freeThrowsAttempted",
            "totalRebounds",
            "assists",
            "turnovers",
            "largestLead",
        ],
        Sport::Hockey => &["shotsTotal", "powerPlayGoals", "hits", "blockedShots", "faceoffsWon", "penaltyMinutes"],
        Sport::Soccer => &["possessionPct", "totalShots", "shotsOnTarget", "wonCorners", "foulsCommitted", "saves"],
    }
}

/// Most rows a panel shows (they must fit next to the game view).
const MAX_ROWS: usize = 7;

fn period_label(sport: Sport, period: u8) -> String {
    match sport {
        Sport::Football | Sport::Basketball if period <= 4 => format!("Q{period}"),
        Sport::Football | Sport::Basketball => "OT".into(),
        Sport::Hockey if period <= 3 => format!("P{period}"),
        Sport::Hockey => "OT".into(),
        Sport::Soccer => {
            if period <= 2 {
                format!("{period}H")
            } else {
                "ET".into()
            }
        }
        Sport::Baseball => format!("INN {period}"),
    }
}

/// What the spotlight shows for `game`: team stats, leaders and the latest
/// scoring plays, each a panel of (away, label, home) or (when, what, score)
/// rows. Empty panels are left out.
pub fn panels(summary: &GameSummary, game: &Game) -> Vec<StatPanel> {
    let mut out = Vec::new();
    let mut stats: Vec<[String; 3]> = key_stats(game.sport)
        .iter()
        .filter_map(|key| summary.team_stats.iter().find(|s| s.name == *key))
        .map(|s| [s.away.clone(), s.label.to_uppercase(), s.home.clone()])
        .collect();
    if stats.is_empty() {
        stats = summary.team_stats.iter().map(|s| [s.away.clone(), s.label.to_uppercase(), s.home.clone()]).collect();
    }
    if let Some(home) = summary.home_win.filter(|_| game.status.is_live()) {
        stats.insert(0, [format!("{}%", 100 - home.min(100)), "WIN CHANCE".into(), format!("{home}%")]);
    }
    stats.truncate(MAX_ROWS);
    if !stats.is_empty() {
        out.push(StatPanel { title: "TEAM STATS".into(), rows: stats, text_rows: false });
    }
    let leaders: Vec<[String; 3]> = summary
        .leaders
        .iter()
        .filter(|l| l.away.is_some() || l.home.is_some())
        .take(4)
        .map(|l| {
            [
                l.away.clone().unwrap_or_else(|| "–".into()),
                l.category.to_uppercase(),
                l.home.clone().unwrap_or_else(|| "–".into()),
            ]
        })
        .collect();
    if !leaders.is_empty() {
        out.push(StatPanel { title: "LEADERS".into(), rows: leaders, text_rows: false });
    }
    let scoring: Vec<[String; 3]> = summary
        .scoring
        .iter()
        .rev()
        .take(MAX_ROWS)
        .map(|p| {
            let who = match (&p.team, &p.kind) {
                (Some(t), Some(k)) => format!("{t} {k} · "),
                (Some(t), None) => format!("{t} · "),
                _ => String::new(),
            };
            [
                format!("{} {}", period_label(game.sport, p.period), p.clock).trim().to_owned(),
                format!("{who}{}", p.text),
                format!("{}-{}", p.away_score, p.home_score),
            ]
        })
        .collect();
    if !scoring.is_empty() {
        out.push(StatPanel { title: "SCORING".into(), rows: scoring, text_rows: true });
    }
    out
}

/// Scoring plays the ticker tells, newest first.
const STORY_PLAYS: usize = 3;
/// Team stats the ticker tells (besides the win chance).
const STORY_STATS: usize = 3;

/// The spotlighted game's story for the ticker, to follow its score: one
/// segment with the win chance (while live) and a few key stats, then the
/// latest scoring plays, newest first. Short stacked lines, like the game's
/// own segment. Ids are `story:<game id>:stats` and
/// `story:<game id>:score:<n>` (`n` counts plays from the first, so a play
/// keeps its id as more come in).
pub fn story_segments(summary: &GameSummary, game: &Game) -> Vec<TickerSegment> {
    let (away, home) = (&game.away.team.abbreviation, &game.home.team.abbreviation);
    let stack = |top: Vec<Span>, bottom: Vec<Span>| Part::stack(top, bottom, Align::Left);
    let mut out = Vec::new();

    let mut stats = Vec::new();
    if let Some(home_win) = summary.home_win.filter(|_| game.status.is_live()).map(|h| h.min(100)) {
        let (team, pct) = if home_win >= 50 { (home, home_win) } else { (away, 100 - home_win) };
        stats.push(stack(vec![Span::dim("WIN CHANCE")], vec![Span::primary(format!("{team} {pct}%"))]));
    }
    let picked = key_stats(game.sport)
        .iter()
        .filter_map(|key| summary.team_stats.iter().find(|s| s.name == *key))
        .take(STORY_STATS);
    for s in picked {
        stats.push(stack(
            vec![Span::dim(s.label.to_uppercase())],
            vec![Span::primary(format!("{}-{}", s.away, s.home))],
        ));
    }
    if !stats.is_empty() {
        let mut parts = Vec::new();
        for (i, p) in stats.into_iter().enumerate() {
            if i > 0 {
                parts.push(Part::gap(6));
            }
            parts.push(p);
        }
        out.push(TickerSegment { id: format!("story:{}:stats", game.id.0), parts });
    }

    for (n, p) in summary.scoring.iter().enumerate().rev().take(STORY_PLAYS) {
        let what = [p.team.as_deref(), p.kind.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
        let when = format!("{} {}", period_label(game.sport, p.period), p.clock).trim().to_owned();
        let parts = vec![
            stack(
                vec![Span::new(if what.is_empty() { "SCORE".into() } else { what }, Tint::Accent)],
                vec![Span::dim(when)],
            ),
            Part::gap(4),
            stack(vec![Span::primary(away.clone())], vec![Span::primary(home.clone())]),
            Part::gap(3),
            Part::stack(
                vec![Span::primary(p.away_score.to_string())],
                vec![Span::primary(p.home_score.to_string())],
                Align::Right,
            ),
        ];
        out.push(TickerSegment { id: format!("story:{}:score:{n}", game.id.0), parts });
    }
    out
}

/// True when a summary is worth fetching again (the game's still going, or
/// it has none yet).
pub fn needs_refresh(game: &Game, have: bool) -> bool {
    !have || matches!(game.status, GameStatus::InProgress | GameStatus::Halftime | GameStatus::Delayed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures::mock_games;
    use chrono::Utc;

    fn stat(name: &str, label: &str, a: &str, h: &str) -> TeamStat {
        TeamStat { name: name.into(), label: label.into(), away: a.into(), home: h.into() }
    }

    fn football() -> Game {
        mock_games(Utc::now()).into_iter().find(|g| g.sport == Sport::Football && g.status.is_live()).unwrap()
    }

    #[test]
    fn football_panels_pick_the_key_stats_in_order() {
        let summary = GameSummary {
            at_bat: None,
            team_stats: vec![
                stat("firstDowns", "1st Downs", "18", "15"),
                stat("totalYards", "Total Yards", "340", "287"),
                stat("yardsPerPlay", "Yards per Play", "5.7", "4.9"),
            ],
            leaders: vec![LeaderRow { category: "Passing Yards".into(), away: Some("A 212 YDS".into()), home: None }],
            scoring: vec![
                ScoringPlay {
                    period: 1,
                    clock: "9:12".into(),
                    team: Some("KC".into()),
                    kind: Some("TD".into()),
                    text: "run".into(),
                    away_score: 7,
                    home_score: 0,
                },
                ScoringPlay {
                    period: 3,
                    clock: "2:58".into(),
                    team: Some("BUF".into()),
                    kind: Some("FG".into()),
                    text: "kick".into(),
                    away_score: 7,
                    home_score: 3,
                },
            ],
            home_win: Some(72),
            drive: Default::default(),
        };
        let p = panels(&summary, &football());
        let titles: Vec<&str> = p.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["TEAM STATS", "LEADERS", "SCORING"]);
        assert_eq!(p[0].rows[0], ["28%", "WIN CHANCE", "72%"].map(String::from), "live: win chance first");
        assert_eq!(p[0].rows[1][1], "TOTAL YARDS");
        assert_eq!(p[0].rows[2][1], "1ST DOWNS", "yards per play isn't a key stat");
        assert_eq!(p[1].rows[0], ["A 212 YDS", "PASSING YARDS", "–"].map(String::from));
        assert_eq!(p[2].rows[0], ["Q3 2:58", "BUF FG · kick", "7-3"].map(String::from), "newest first");
    }

    #[test]
    fn unknown_stats_still_show_and_empty_panels_are_dropped() {
        let summary = GameSummary { team_stats: vec![stat("odd", "Odd Stat", "1", "2")], ..GameSummary::default() };
        let p = panels(&summary, &football());
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].rows[0][1], "ODD STAT");
        assert!(panels(&GameSummary::default(), &football()).is_empty());
    }

    #[test]
    fn the_story_tells_stats_then_the_latest_scores() {
        use crate::sports::fixtures::mock_summary;
        use crate::sports::ticker::segment_text;
        let game = football();
        let segs = story_segments(&mock_summary(), &game);
        let id = &game.id.0;
        let ids: Vec<&str> = segs.iter().map(|s| s.id.as_str()).collect();
        let score = |n: usize| format!("story:{id}:score:{n}");
        assert_eq!(ids, [format!("story:{id}:stats"), score(5), score(4), score(3)], "newest plays first");
        let (away, home) = (&game.away.team.abbreviation, &game.home.team.abbreviation);
        assert_eq!(
            segment_text(&segs[0]),
            format!("WIN CHANCE/{home} 64% TOTAL YARDS/298-341 PASSING/201-212 RUSHING/97-129")
        );
        assert_eq!(segment_text(&segs[1]), format!("BUF TD/Q3 4:58 {away}/{home} 17/21"));
    }

    #[test]
    fn a_finished_game_has_no_win_chance_and_no_summary_no_story() {
        use crate::sports::fixtures::mock_summary;
        use crate::sports::ticker::segment_text;
        let mut game = football();
        game.status = GameStatus::Final;
        let segs = story_segments(&mock_summary(), &game);
        assert!(segment_text(&segs[0]).starts_with("TOTAL YARDS/"));
        assert!(story_segments(&GameSummary::default(), &game).is_empty());
    }

    #[test]
    fn finished_games_stop_refreshing() {
        let mut g = football();
        assert!(needs_refresh(&g, true));
        g.status = GameStatus::Final;
        assert!(!needs_refresh(&g, true) && needs_refresh(&g, false));
    }
}
