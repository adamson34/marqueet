//! Turns stored games into what the display shows. Pure.

use marqueet_core::protocol::Content;
use marqueet_core::sports::ticker::{FormatOptions, crawl_segments, ticker_segments};
use marqueet_core::ticker::{Part, Span, TickerSegment, Tint};
use marqueet_core::widgets::default_views;

use crate::store::Store;

fn notice(id: &str, text: &str) -> TickerSegment {
    TickerSegment { id: id.into(), parts: vec![Part::text(vec![Span::new(text, Tint::Dim)])] }
}

pub fn build(store: &Store, opts: &FormatOptions) -> Content {
    let games = store.games();
    let mut ticker = ticker_segments(&games, opts);

    // Flag leagues whose data is old because fetches keep failing.
    for league in store.leagues().iter().filter(|l| store.is_stale(l)) {
        let header_id = format!("league:{league}");
        if let Some(Part::Text { spans }) =
            ticker.iter_mut().find(|s| s.id == header_id).and_then(|s| s.parts.first_mut())
        {
            spans.push(Span::new(" DELAYED", Tint::Dim));
        }
    }

    if ticker.is_empty() {
        let text = if store.is_empty_startup() { "LOADING SCORES" } else { "NO GAMES TODAY" };
        ticker.push(notice("status:empty", text));
    }
    let mut crawl = crawl_segments(&games, opts);
    if crawl.is_empty() {
        crawl.push(notice("status:crawl", "NO UPCOMING GAMES"));
    }
    let widgets = default_views(&games, &[], opts.tz, opts.now);
    Content { ticker, crawl, status: store.status(), widgets }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};
    use marqueet_core::sports::LeagueId;
    use marqueet_core::sports::fixtures::mock_games;
    use marqueet_core::sports::ticker::segment_text;

    fn opts() -> FormatOptions {
        FormatOptions { tz: FixedOffset::east_opt(0).unwrap(), now: Utc::now() }
    }

    fn nfl() -> LeagueId {
        LeagueId::new("nfl")
    }

    #[test]
    fn startup_and_empty_day_notices() {
        let mut s = Store::new(vec![nfl()], 3);
        assert_eq!(segment_text(&build(&s, &opts()).ticker[0]), "LOADING SCORES");
        s.record_success(&nfl(), Vec::new(), Utc::now());
        let c = build(&s, &opts());
        assert_eq!(segment_text(&c.ticker[0]), "NO GAMES TODAY");
        assert_eq!(segment_text(&c.crawl[0]), "NO UPCOMING GAMES");
    }

    #[test]
    fn stale_league_header_says_delayed() {
        let mut s = Store::new(vec![nfl()], 1);
        let games = mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect();
        s.record_success(&nfl(), games, Utc::now());
        assert_eq!(segment_text(&build(&s, &opts()).ticker[0]), "NFL");
        s.record_failure(&nfl(), "HTTP 503".into());
        let c = build(&s, &opts());
        assert_eq!(segment_text(&c.ticker[0]), "NFL DELAYED");
        assert_eq!(c.status.stale_leagues, vec!["nfl".to_string()]);
        assert!(c.ticker.len() > 1, "stale games are still shown");
    }
}
