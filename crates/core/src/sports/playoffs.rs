//! Playoff series and brackets, built from playoff games (each carries its
//! [`SeriesInfo`]). Pure: the server collects a postseason's games; this
//! groups them into series and lays the series out as a bracket.

use chrono::{DateTime, Utc};

use super::{Game, GameId, GameStatus, LeagueId, TeamId};
use crate::color::Rgb;

/// One team in a series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeriesTeam {
    pub id: TeamId,
    pub abbr: String,
    pub color: Rgb,
    pub wins: u8,
}

/// A playoff series and where it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Series {
    pub league: LeagueId,
    /// "ALDS", "World Series".
    pub round: String,
    pub side: Option<String>,
    pub stage: u8,
    pub best_of: u8,
    /// The home team of the first game (usually the higher seed) first.
    pub teams: [SeriesTeam; 2],
    pub completed: bool,
    /// A game of the series in progress.
    pub live: Option<GameId>,
    /// The next scheduled game: its number and start.
    pub next: Option<(Option<u8>, DateTime<Utc>)>,
    /// When the series' first game started (for ordering).
    pub first_game: DateTime<Utc>,
}

impl Series {
    /// Wins needed to take the series.
    pub fn to_win(&self) -> u8 {
        self.best_of / 2 + 1
    }

    /// The team that has won it.
    pub fn winner(&self) -> Option<&SeriesTeam> {
        self.teams.iter().find(|t| t.wins >= self.to_win())
    }

    fn has(&self, team: &TeamId) -> bool {
        self.teams.iter().any(|t| &t.id == team)
    }
}

/// Groups playoff games into series (a series is a stage plus a pair of
/// teams). Wins are the highest either game reports, so a game fetched before
/// another was played can't take a win back.
pub fn series_list(games: &[Game]) -> Vec<Series> {
    let mut playoff: Vec<&Game> = games.iter().filter(|g| g.series.is_some()).collect();
    playoff.sort_by_key(|g| g.start_time);
    let mut out: Vec<Series> = Vec::new();
    for g in playoff {
        let Some(info) = &g.series else { continue };
        let (home, away) = (&g.home.team, &g.away.team);
        let found = out.iter_mut().find(|s| {
            s.league == g.league && s.stage == info.stage && s.round == info.round && s.has(&home.id) && s.has(&away.id)
        });
        let s = match found {
            Some(s) => s,
            None => {
                let team = |t: &super::Team| SeriesTeam {
                    id: t.id.clone(),
                    abbr: t.abbreviation.clone(),
                    color: t.colors.primary,
                    wins: 0,
                };
                out.push(Series {
                    league: g.league.clone(),
                    round: info.round.clone(),
                    side: info.side.clone(),
                    stage: info.stage,
                    best_of: info.best_of,
                    teams: [team(home), team(away)],
                    completed: false,
                    live: None,
                    next: None,
                    first_game: g.start_time,
                });
                let last = out.len() - 1;
                &mut out[last]
            }
        };
        for (id, wins) in [(&home.id, info.home_wins), (&away.id, info.away_wins)] {
            if let Some(t) = s.teams.iter_mut().find(|t| &t.id == id) {
                t.wins = t.wins.max(wins);
            }
        }
        s.completed |= info.completed;
        s.best_of = s.best_of.max(info.best_of);
        if g.status.is_live() {
            s.live = Some(g.id.clone());
        }
        if g.status == GameStatus::Scheduled && s.next.is_none_or(|(_, at)| g.start_time < at) {
            s.next = Some((info.game_number, g.start_time));
        }
    }
    for s in &mut out {
        if s.winner().is_some() {
            s.completed = true;
        }
        if s.completed {
            s.next = None;
        }
    }
    out
}

/// One round of a bracket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub stage: u8,
    pub title: String,
    /// In bracket order: each series sits level with the ones feeding it.
    pub series: Vec<Series>,
}

/// A league's bracket: one column per round, first round first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bracket {
    pub league: LeagueId,
    pub columns: Vec<Column>,
}

/// Round titles for leagues whose format is known; others get the provider's
/// round names.
fn stage_titles(league: &LeagueId) -> &'static [&'static str] {
    match league.as_str() {
        "mlb" => &["WILD CARD", "DIVISION SERIES", "LCS", "WORLD SERIES"],
        "nba" | "wnba" | "nhl" => &["FIRST ROUND", "SECOND ROUND", "CONF FINALS", "FINALS"],
        _ => &[],
    }
}

/// The bracket for `league` from its playoff games, or `None` before any.
pub fn bracket(games: &[Game], league: &LeagueId) -> Option<Bracket> {
    let all: Vec<Series> = series_list(games).into_iter().filter(|s| &s.league == league && s.stage > 0).collect();
    if all.is_empty() {
        return None;
    }
    let titles = stage_titles(league);
    let top = all.iter().map(|s| s.stage).max().unwrap_or(1).max(u8::try_from(titles.len()).unwrap_or(0));

    // A series' feeders: the previous round's series its teams came from.
    let feeders = |s: &Series| -> Vec<usize> {
        let mut out = Vec::new();
        for team in &s.teams {
            if let Some(i) = all.iter().position(|f| f.stage + 1 == s.stage && f.has(&team.id)) {
                out.push(i);
            }
        }
        out
    };
    let fed: Vec<bool> = (0..all.len()).map(|i| all.iter().any(|s| feeders(s).contains(&i))).collect();
    // Roots (series nothing later grew from yet): one bracket half after
    // the other, the latest round first.
    let mut roots: Vec<usize> = (0..all.len()).filter(|i| !fed[*i]).collect();
    let side_rank = |s: &Series| (s.side.is_none(), s.side.clone());
    roots.sort_by(|a, b| {
        let (a, b) = (&all[*a], &all[*b]);
        (side_rank(a), std::cmp::Reverse(a.stage), a.first_game).cmp(&(
            side_rank(b),
            std::cmp::Reverse(b.stage),
            b.first_game,
        ))
    });

    let mut placed: Vec<Vec<usize>> = vec![Vec::new(); usize::from(top)];
    fn visit(i: usize, all: &[Series], feeders: &dyn Fn(&Series) -> Vec<usize>, placed: &mut [Vec<usize>]) {
        for f in feeders(&all[i]) {
            visit(f, all, feeders, placed);
        }
        let stage = usize::from(all[i].stage);
        if let Some(col) = placed.get_mut(stage - 1)
            && !col.contains(&i)
        {
            col.push(i);
        }
    }
    for r in roots {
        visit(r, &all, &feeders, &mut placed);
    }

    let columns = placed
        .into_iter()
        .enumerate()
        .map(|(i, idx)| {
            let series: Vec<Series> = idx.into_iter().map(|j| all[j].clone()).collect();
            let title = titles.get(i).map(|t| (*t).to_owned()).unwrap_or_else(|| match series.first() {
                Some(s) if s.side.is_none() => s.round.to_uppercase(),
                _ => format!("ROUND {}", i + 1),
            });
            Column { stage: u8::try_from(i + 1).unwrap_or(u8::MAX), title, series }
        })
        .collect();
    Some(Bracket { league: league.clone(), columns })
}

/// The leagues with playoff games, most recently played first.
pub fn leagues_in_playoffs(games: &[Game]) -> Vec<LeagueId> {
    let mut latest: Vec<(LeagueId, DateTime<Utc>)> = Vec::new();
    for g in games.iter().filter(|g| g.series.is_some()) {
        match latest.iter_mut().find(|(l, _)| l == &g.league) {
            Some((_, at)) => *at = (*at).max(g.start_time),
            None => latest.push((g.league.clone(), g.start_time)),
        }
    }
    latest.sort_by_key(|l| std::cmp::Reverse(l.1));
    latest.into_iter().map(|(l, _)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sports::fixtures::mock_playoff_games;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 12, 18, 0, 0).unwrap()
    }

    fn mlb() -> LeagueId {
        LeagueId::new("mlb")
    }

    #[test]
    fn games_group_into_series_with_the_latest_wins() {
        let list = series_list(&mock_playoff_games(now()));
        let wc: Vec<&Series> = list.iter().filter(|s| s.stage == 1).collect();
        assert_eq!(wc.len(), 4);
        assert!(wc.iter().all(|s| s.completed && s.winner().is_some()), "wild card is over");
        let live = list.iter().find(|s| s.live.is_some()).unwrap();
        assert_eq!(live.stage, 3, "an LCS game is on");
        assert!(!live.completed && live.winner().is_none());
        assert!(list.iter().any(|s| s.next.is_some()), "a scheduled game shows as next");
    }

    #[test]
    fn the_bracket_puts_each_series_level_with_its_feeders() {
        let b = bracket(&mock_playoff_games(now()), &mlb()).unwrap();
        let titles: Vec<&str> = b.columns.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["WILD CARD", "DIVISION SERIES", "LCS", "WORLD SERIES"]);
        let counts: Vec<usize> = b.columns.iter().map(|c| c.series.len()).collect();
        assert_eq!(counts, [4, 4, 2, 0], "no World Series yet");
        // Each LCS team came from the division series at the same place.
        for (i, lcs) in b.columns[2].series.iter().enumerate() {
            let feeders = &b.columns[1].series[i * 2..i * 2 + 2];
            for f in feeders {
                let w = f.winner().unwrap();
                assert!(lcs.has(&w.id), "{} fed the LCS in row {i}", w.abbr);
            }
        }
        // AL above NL.
        assert_eq!(b.columns[2].series[0].side.as_deref(), Some("AL"));
        assert!(bracket(&[], &mlb()).is_none());
    }

    #[test]
    fn a_league_in_its_playoffs_is_found() {
        assert_eq!(leagues_in_playoffs(&mock_playoff_games(now())), [mlb()]);
        assert!(leagues_in_playoffs(&crate::sports::fixtures::mock_games(now())).is_empty());
    }
}
