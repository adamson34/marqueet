//! Turns stored games into what the display shows. Pure.

use marqueet_core::feeds::Position;
use marqueet_core::protocol::Content;
use marqueet_core::settings::Settings;
use marqueet_core::sports::ticker::{FormatOptions, crawl_label, crawl_segments, ticker_segments};
use marqueet_core::ticker::{Part, Span, TickerSegment, Tint};
use marqueet_core::weather;
use marqueet_core::widgets::{WidgetData, build_views};

use crate::store::Store;

fn notice(id: &str, text: &str) -> TickerSegment {
    TickerSegment { id: id.into(), parts: vec![Part::text(vec![Span::new(text, Tint::Dim)])] }
}

pub fn build(store: &Store, opts: &FormatOptions, settings: &Settings) -> Content {
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
    let standings = store.standings();
    let weather = settings.weather.place.as_ref().and_then(|p| store.weather_for(p, settings.weather.units));
    // Custom feeds: "start" ones right after the weather, "end" ones last.
    let feeds: Vec<_> = store.custom_feeds(opts.now).collect();
    let mut first: Vec<TickerSegment> = Vec::new();
    for feed in &feeds {
        match feed.position {
            Position::Start => first.extend(feed.segments.iter().cloned()),
            Position::End => ticker.extend(feed.segments.iter().cloned()),
        }
    }
    let feed_crawl: Vec<TickerSegment> = feeds.iter().flat_map(|f| f.crawl.iter().cloned()).collect();
    if !feed_crawl.is_empty() {
        crawl.retain(|s| s.id != "status:crawl");
        crawl.extend(feed_crawl);
    }
    // Weather leads each ticker loop (ticker first; the widget is extra).
    if settings.weather.ticker
        && let Some(w) = weather
    {
        first.insert(0, weather::ticker_segment(w));
    }
    ticker.splice(0..0, first);
    let data = WidgetData { games: &games, standings: &standings, weather, favorites: &settings.favorites };
    let widgets = build_views(&settings.widgets, &data, opts.tz, opts.now);
    Content { ticker, crawl, crawl_label: crawl_label(&games, opts), status: store.status(), widgets }
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
        assert_eq!(segment_text(&build(&s, &opts(), &Settings::default()).ticker[0]), "LOADING SCORES");
        s.record_success(&nfl(), Vec::new(), Utc::now());
        let c = build(&s, &opts(), &Settings::default());
        assert_eq!(segment_text(&c.ticker[0]), "NO GAMES TODAY");
        assert_eq!(segment_text(&c.crawl[0]), "NO UPCOMING GAMES");
    }

    #[test]
    fn weather_leads_the_ticker_when_enabled() {
        use marqueet_core::weather::mock_weather;
        let w = mock_weather(Utc::now());
        let mut s = Store::new(vec![nfl()], 3);
        s.record_success(&nfl(), Vec::new(), Utc::now());
        s.record_weather(Ok(w.clone()), Utc::now());
        let mut settings = Settings::default();
        assert_eq!(build(&s, &opts(), &settings).ticker[0].id, "status:empty", "no place set");
        settings.weather.place = Some(w.place.clone());
        let c = build(&s, &opts(), &settings);
        assert_eq!(c.ticker[0].id, "weather");
        assert_eq!(segment_text(&c.ticker[1]), "NO GAMES TODAY");
        settings.weather.ticker = false;
        assert!(build(&s, &opts(), &settings).ticker.iter().all(|t| t.id != "weather"));
    }

    #[test]
    fn custom_feeds_join_the_ticker_and_crawl_until_they_expire() {
        use marqueet_core::feeds::{FeedPost, accept};
        let now = Utc::now();
        let mut s = Store::new(vec![nfl()], 3);
        s.record_success(&nfl(), mock_games(now).into_iter().filter(|g| g.league.as_str() == "nfl").collect(), now);
        let post = |json: &str| serde_json::from_str::<FeedPost>(json).unwrap();
        s.set_custom_feed(
            accept("stocks", &post(r#"{"segments":[{"text":"AAPL"}],"crawl":[{"text":"MKTS CLOSE 4PM"}]}"#), now)
                .unwrap(),
        );
        s.set_custom_feed(
            accept("alerts", &post(r#"{"segments":[{"text":"ON CALL"}],"position":"start","ttl":30}"#), now).unwrap(),
        );
        let c = build(&s, &opts(), &Settings::default());
        assert_eq!(c.ticker[0].id, "feed:alerts:0", "start feeds lead");
        assert_eq!(c.ticker.last().unwrap().id, "feed:stocks:0", "end feeds close the loop");
        assert_eq!(c.crawl.last().unwrap().id, "feed:stocks:0:crawl");

        let later = FormatOptions { now: now + chrono::Duration::seconds(31), ..opts() };
        let c = build(&s, &later, &Settings::default());
        assert!(c.ticker.iter().all(|t| t.id != "feed:alerts:0"), "expired");
        assert!(s.prune_custom_feeds(now + chrono::Duration::seconds(31)));
        assert!(s.remove_custom_feed("stocks") && !s.remove_custom_feed("stocks"));
    }

    #[test]
    fn stale_league_header_says_delayed() {
        let mut s = Store::new(vec![nfl()], 1);
        let games = mock_games(Utc::now()).into_iter().filter(|g| g.league.as_str() == "nfl").collect();
        s.record_success(&nfl(), games, Utc::now());
        assert_eq!(segment_text(&build(&s, &opts(), &Settings::default()).ticker[0]), "NFL");
        s.record_failure(&nfl(), "HTTP 503".into());
        let c = build(&s, &opts(), &Settings::default());
        assert_eq!(segment_text(&c.ticker[0]), "NFL DELAYED");
        assert_eq!(c.status.stale_leagues, vec!["nfl".to_string()]);
        assert!(c.ticker.len() > 1, "stale games are still shown");
    }
}
