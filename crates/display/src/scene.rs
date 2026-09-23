//! Everything on screen, independent of the GPU: layout, the LED panels
//! (ticker, crawl, welcome logo, placeholder clock) and the mock data
//! driving them.

use chrono::{DateTime, FixedOffset, Utc};
use marqueet_core::alert::Alert;
use marqueet_core::color::led_team_color;
use marqueet_core::config::DisplayConfig;
use marqueet_core::layout::{LedGrid, Rect, ScreenLayout};
use marqueet_core::logo::DotMark;
use marqueet_core::sports::ticker::{FormatOptions, crawl_segments, ticker_segments};
use marqueet_core::ticker::{Align, LedBitmap, Palette, Part, RasterStyle, Rasterizer, Span, TickerSegment, Tint};

use crate::band::Band;
use crate::feed::{FeedEvent, LiveFeed};
use crate::mock::MockFeed;
use marqueet_core::protocol::{Content, ServerMsg};
use marqueet_core::sports::HomeAway;

/// Clock panel width in LEDs; fixed so the dot size never changes.
const CLOCK_COLS: u32 = 112;
/// How long the welcome logo shows at startup, and how long its dots take to
/// sweep on.
pub const WELCOME_SECS: f64 = 4.0;
const WELCOME_REVEAL_SECS: f64 = 0.9;

#[derive(Debug)]
pub struct Panel {
    pub band: Band,
    pub grid: LedGrid,
    pub visible: bool,
}

#[derive(Debug)]
pub struct Scene {
    pub config: DisplayConfig,
    pub layout: ScreenLayout,
    pub panels: Vec<Panel>,
    feed: Feed,
    tz: FixedOffset,
    /// Seconds since the scene started.
    pub time: f64,
    max_strip_width: u32,
}

/// Where ticker content comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedSource {
    /// Built-in demo data (no server needed).
    Mock { seed: u64 },
    /// `marqueet-server` at this WebSocket URL.
    Live { url: String },
}

#[derive(Debug)]
enum Feed {
    Mock(MockFeed),
    Live {
        client: LiveFeed,
        url: String,
        /// Latest content from the server; kept while disconnected.
        content: Option<Content>,
        connected: bool,
        /// Content changed (or panels were rebuilt) since it was last applied.
        dirty: bool,
    },
}

const TICKER: usize = 0;
const CLOCK: usize = 1;
const WELCOME: usize = 2;
const CRAWL: usize = 3;

/// How a scene starts: clock, time zone, data source and GPU limits.
#[derive(Debug, Clone)]
pub struct SceneSetup {
    pub now: DateTime<Utc>,
    pub tz: FixedOffset,
    pub source: FeedSource,
    /// Longest strip the GPU can hold (see `Renderer::max_strip_width`).
    pub max_strip_width: u32,
}

impl Scene {
    pub fn new(config: DisplayConfig, width: u32, height: u32, setup: SceneSetup) -> Self {
        let SceneSetup { now, tz, source, max_strip_width } = setup;
        let feed = match source {
            FeedSource::Mock { seed } => Feed::Mock(MockFeed::new(now, seed)),
            FeedSource::Live { url } => {
                log::info!("connecting to {url}");
                Feed::Live { client: LiveFeed::connect(&url), url, content: None, connected: false, dirty: true }
            }
        };
        let mut scene = Scene {
            layout: compute_layout(&config, width, height),
            config,
            panels: Vec::new(),
            feed,
            tz,
            time: 0.0,
            max_strip_width,
        };
        scene.rebuild_panels(now);
        scene
    }

    pub fn resize(&mut self, width: u32, height: u32, now: DateTime<Utc>) {
        if (width, height) != (self.layout.width, self.layout.height) {
            self.layout = compute_layout(&self.config, width, height);
            self.rebuild_panels(now);
        }
    }

    fn rebuild_panels(&mut self, now: DateTime<Utc>) {
        let c = &self.config;
        let palette = Palette::new(c.led_color);
        let l = self.layout;
        let max = self.max_strip_width;
        let mut panels = vec![
            Panel {
                band: Band::new(
                    Rasterizer::new(l.ticker.rows, palette),
                    l.ticker.cols,
                    f64::from(c.ticker_speed),
                    true,
                    max,
                ),
                grid: l.ticker,
                visible: true,
            },
            {
                let mut rast = Rasterizer::new(c.ticker_rows, palette);
                rast.separator = None;
                let area = inset(l.widgets, 0.72, 0.5);
                Panel {
                    band: Band::new(rast, CLOCK_COLS, 0.0, false, max),
                    grid: LedGrid::fit_centered(area, CLOCK_COLS, c.ticker_rows),
                    visible: false,
                }
            },
            {
                let logo = welcome_bitmap(palette);
                let (cols, rows) = (logo.width + 10, logo.height + 2);
                let mut band = Band::new(Rasterizer::new(rows, palette), cols, 0.0, false, max);
                band.set_bitmap(LedBitmap::new(logo.width, rows));
                Panel { band, grid: LedGrid::fit_centered(inset(l.widgets, 0.8, 0.62), cols, rows), visible: true }
            },
        ];
        if let Some(crawl) = l.crawl {
            panels.push(Panel {
                band: Band::new(Rasterizer::new(crawl.rows, palette), crawl.cols, f64::from(c.crawl_speed), true, max),
                grid: crawl,
                visible: true,
            });
        }
        self.panels = panels;
        if let Feed::Live { dirty, .. } = &mut self.feed {
            *dirty = true;
        }
        self.refresh_content(now);
    }

    fn format_options(&self, now: DateTime<Utc>) -> FormatOptions {
        FormatOptions { tz: self.tz, now }
    }

    fn set_ticker_and_crawl(&mut self, ticker: Vec<TickerSegment>, crawl: Vec<TickerSegment>) {
        self.panels[TICKER].band.set_segments(ticker);
        if let Some(p) = self.panels.get_mut(CRAWL) {
            p.band.set_segments(crawl);
        }
    }

    fn refresh_content(&mut self, now: DateTime<Utc>) {
        let opts = self.format_options(now);
        match &mut self.feed {
            Feed::Mock(feed) => {
                let (ticker, crawl) = (ticker_segments(&feed.games, &opts), crawl_segments(&feed.games, &opts));
                self.set_ticker_and_crawl(ticker, crawl);
            }
            Feed::Live { content, url, dirty, .. } => {
                // Server content arrives ready to render; apply it only when
                // it changed rather than cloning it every frame.
                if std::mem::take(dirty) {
                    let (ticker, crawl) = match content {
                        Some(c) => (c.ticker.clone(), c.crawl.clone()),
                        None => {
                            (vec![notice("status:connecting", "CONNECTING TO SERVER")], vec![notice("status:url", url)])
                        }
                    };
                    self.set_ticker_and_crawl(ticker, crawl);
                }
            }
        }
        let clock = clock_segment(now.with_timezone(&self.tz));
        self.panels[CLOCK].band.set_segments(vec![clock]);

        let welcome = self.time < WELCOME_SECS;
        self.panels[WELCOME].visible = welcome;
        self.panels[CLOCK].visible = !welcome;
        if welcome {
            let logo = welcome_bitmap(Palette::new(self.config.led_color));
            let t = (self.time / WELCOME_REVEAL_SECS).min(1.0);
            let mut framed = LedBitmap::new(logo.width, logo.height + 2);
            framed.blit(&logo.reveal((t * f64::from(logo.width)).ceil() as u32), 0, 1);
            self.panels[WELCOME].band.set_bitmap(framed);
        }
    }

    /// Scores `points` for one side of a mock game and flashes it on the
    /// ticker, exactly as a live scoring alert would (headless `--score`).
    pub fn score(&mut self, game_id: &str, side: HomeAway, points: u16) -> bool {
        let Feed::Mock(feed) = &mut self.feed else { return false };
        let Some(game) = feed.add_points(game_id, side, points) else { return false };
        let team = &game.competitor(side).team.colors;
        let color = led_team_color(team.primary, team.secondary);
        let now = feed.now;
        self.refresh_content(now);
        self.panels[TICKER].band.flash(game_id, color, self.time);
        true
    }

    /// Advances time by `dt` seconds; `now` is the wall clock.
    pub fn update(&mut self, dt: f64, now: DateTime<Utc>) {
        self.time += dt;
        let alerts = match &mut self.feed {
            Feed::Mock(feed) => feed.advance(dt, now).alerts,
            Feed::Live { client, .. } => {
                let events = client.drain();
                let mut alerts = Vec::new();
                for event in events {
                    if let Some(alert) = self.apply_feed_event(event) {
                        alerts.push(alert);
                    }
                }
                alerts
            }
        };
        // Clock text changes once a minute; set_segments skips no-op updates.
        self.refresh_content(now);
        for alert in &alerts {
            self.apply_alert(alert);
        }
        for p in &mut self.panels {
            p.band.update(dt, self.time);
        }
    }

    /// Handles one event from the live feed; returns an alert to show.
    pub(crate) fn apply_feed_event(&mut self, event: FeedEvent) -> Option<Alert> {
        let Feed::Live { content, connected, dirty, url, .. } = &mut self.feed else { return None };
        match event {
            FeedEvent::Connected => {
                *connected = true;
                log::info!("connected to {url}");
            }
            FeedEvent::Disconnected(reason) => {
                if std::mem::replace(connected, false) {
                    log::warn!("lost connection to {url}: {reason}; showing last content and retrying");
                } else {
                    log::debug!("still can't reach {url}: {reason}");
                }
            }
            FeedEvent::Message(ServerMsg::Hello { server_version, .. }) => {
                log::info!("server {server_version}");
            }
            FeedEvent::Message(ServerMsg::Content(c)) => {
                if content.as_ref() != Some(&c) {
                    *content = Some(c);
                    *dirty = true;
                }
            }
            FeedEvent::Message(ServerMsg::Alert(a)) => return Some(a),
        }
        None
    }

    /// True once live content has arrived (always true for mock data).
    pub fn has_content(&self) -> bool {
        match &self.feed {
            Feed::Mock(_) => true,
            Feed::Live { content, .. } => content.is_some(),
        }
    }

    fn apply_alert(&mut self, alert: &Alert) {
        let Some(id) = &alert.segment_id else { return };
        let color = alert
            .colors
            .map(|(primary, secondary)| led_team_color(primary, Some(secondary)))
            .unwrap_or(self.config.led_color);
        self.panels[TICKER].band.flash(id, color, self.time);
        log::info!("{}: {}", alert.title, alert.detail.as_deref().unwrap_or(""));
    }

    /// Scrolls the ticker so segment `id` is near the left edge (screenshots).
    pub fn scroll_ticker_to(&mut self, id: &str) -> bool {
        self.panels[TICKER].band.scroll_to(id)
    }

    /// Starts a flash on a ticker segment as if an alert arrived (screenshots).
    pub fn flash_ticker(&mut self, id: &str) {
        let color = self.config.led_color;
        self.panels[TICKER].band.flash(id, color, self.time);
    }
}

fn compute_layout(c: &DisplayConfig, width: u32, height: u32) -> ScreenLayout {
    ScreenLayout::compute(width, height, c.ticker_ratio, c.crawl_share, c.ticker_rows, c.crawl_rows)
}

/// A centered rect covering `fw` x `fh` of `r`.
fn inset(r: Rect, fw: f32, fh: f32) -> Rect {
    let w = (r.w as f32 * fw) as u32;
    let h = (r.h as f32 * fh) as u32;
    Rect { x: r.x + (r.w - w) / 2, y: r.y + (r.h - h) / 2, w, h }
}

fn notice(id: &str, text: &str) -> TickerSegment {
    TickerSegment { id: id.into(), parts: vec![Part::text(vec![Span::new(text.to_uppercase(), Tint::Dim)])] }
}

/// The parakeet mark followed by the MARQUEET wordmark, 16 LEDs tall.
fn welcome_bitmap(palette: Palette) -> LedBitmap {
    // Crown bars, beak, wing and tail in the LED color; the face and chest
    // in a dim pale cream, so the two read apart on the panel.
    let buff = marqueet_core::Rgb::new(255, 226, 180).scale(0.42);
    let mark = DotMark::mark().to_led_bitmap(palette.primary, buff);
    let mut rast = Rasterizer::new(mark.height, palette);
    rast.separator = None;
    let word = rast.render_segment(
        &TickerSegment { id: "welcome".into(), parts: vec![Part::text(vec![Span::primary("MARQUEET")])] },
        RasterStyle::Normal,
    );
    const GAP: u32 = 8;
    let mut out = LedBitmap::new(mark.width + GAP + word.width, mark.height);
    out.blit(&mark, 0, 0);
    out.blit(&word, mark.width + GAP, 0);
    out
}

/// "7:42" large, with "PM" over "TUE 23" beside it.
fn clock_segment(local: DateTime<FixedOffset>) -> TickerSegment {
    TickerSegment {
        id: "clock".into(),
        parts: vec![
            Part::text(vec![Span::primary(local.format("%-I:%M").to_string())]),
            Part::gap(4),
            Part::stack(
                vec![Span::primary(local.format("%p").to_string())],
                vec![Span::new(local.format("%a %-d").to_string().to_uppercase(), Tint::Dim)],
                Align::Left,
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn setup(now: DateTime<Utc>, tz: FixedOffset) -> SceneSetup {
        SceneSetup { now, tz, source: FeedSource::Mock { seed: 1 }, max_strip_width: 200_000 }
    }

    fn scene(w: u32, h: u32) -> Scene {
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        Scene::new(DisplayConfig::default(), w, h, setup(now, FixedOffset::west_opt(4 * 3600).unwrap()))
    }

    #[test]
    fn builds_panels_with_content() {
        let mut s = scene(1920, 1080);
        assert_eq!(s.panels.len(), 4);
        s.update(WELCOME_REVEAL_SECS, Utc::now());
        for p in &s.panels {
            assert!(p.band.strip.bitmap.data.chunks(4).any(|px| px[3] != 0), "panel has lit LEDs");
        }
    }

    #[test]
    fn welcome_logo_sweeps_on_then_hands_over_to_the_clock() {
        let mut s = scene(1920, 1080);
        let lit = |s: &Scene| s.panels[WELCOME].band.strip.bitmap.data.chunks(4).filter(|px| px[3] != 0).count();
        assert!(s.panels[WELCOME].visible && !s.panels[CLOCK].visible);
        s.update(WELCOME_REVEAL_SECS / 3.0, Utc::now());
        let partial = lit(&s);
        s.update(WELCOME_REVEAL_SECS, Utc::now());
        assert!(lit(&s) > partial && partial > 0, "dots sweep on");
        s.update(WELCOME_SECS, Utc::now());
        assert!(!s.panels[WELCOME].visible && s.panels[CLOCK].visible);
    }

    #[test]
    fn scripted_score_updates_the_game_and_flashes_it() {
        let mut s = scene(1920, 1080);
        s.panels[TICKER].band.take_uploads();
        assert!(s.score("mock:nfl:1", HomeAway::Home, 7));
        s.update(0.0, Utc::now());
        let Feed::Mock(feed) = &s.feed else { panic!() };
        let game = feed.games.iter().find(|g| g.id.0 == "mock:nfl:1").unwrap();
        assert_eq!(game.home.score, Some(28));
        assert!(s.panels[TICKER].band.is_flashing("mock:nfl:1"));
        assert!(!s.score("mock:nope", HomeAway::Home, 7));
    }

    #[test]
    fn welcome_fits_every_resolution() {
        for (w, h) in [(1920, 1080), (1366, 768), (1024, 768), (800, 480)] {
            let s = scene(w, h);
            let g = s.panels[WELCOME].grid;
            assert!(g.band.bottom() <= h && g.band.x + g.band.w <= w, "{w}x{h}");
            assert!(g.pitch >= 3, "{w}x{h}");
        }
    }

    #[test]
    fn clock_fits_its_panel_at_every_resolution() {
        for (w, h) in [(1920, 1080), (1366, 768), (1024, 768), (800, 480)] {
            let s = scene(w, h);
            let clock = &s.panels[CLOCK];
            assert!(clock.band.strip.width() <= CLOCK_COLS, "{w}x{h}");
            assert!(clock.grid.band.bottom() <= h);
            assert!(clock.grid.pitch >= 3, "{w}x{h}");
        }
    }

    #[test]
    fn clock_text() {
        let t = FixedOffset::west_opt(4 * 3600).unwrap().with_ymd_and_hms(2026, 9, 23, 19, 5, 0).unwrap();
        let text = marqueet_core::sports::ticker::segment_text(&clock_segment(t));
        assert_eq!(text, "7:05 PM/WED 23");
    }

    #[test]
    fn resize_relayouts() {
        let now = Utc::now();
        let mut s = scene(1920, 1080);
        s.resize(1024, 768, now);
        assert_eq!(s.layout.width, 1024);
        assert_eq!(s.panels[TICKER].grid.cols, s.panels[TICKER].band.cols);
    }

    #[test]
    fn mock_scores_flash_the_ticker() {
        let mut s = scene(1920, 1080);
        let now = Utc::now();
        let mut flashed = false;
        for _ in 0..(60 * 6) {
            s.update(1.0 / 60.0, now);
            flashed |= s.panels[TICKER].band.uploads.iter().any(|u| matches!(u, crate::band::Upload::Columns { .. }));
            s.panels[TICKER].band.take_uploads();
        }
        assert!(flashed, "first mock score (t=4s) should flash a segment");
    }

    #[test]
    fn live_scene_shows_connecting_then_server_content_and_keeps_it_on_disconnect() {
        use marqueet_core::protocol::FeedStatus;
        use marqueet_core::sports::ticker::segment_text;
        let now = Utc::now();
        let source = FeedSource::Live { url: "ws://127.0.0.1:9/ws".into() };
        let mut s = Scene::new(
            DisplayConfig::default(),
            1920,
            1080,
            SceneSetup { now, tz: FixedOffset::east_opt(0).unwrap(), source, max_strip_width: 200_000 },
        );
        let ticker_text =
            |s: &Scene| s.panels[TICKER].band.strip.spans.iter().map(|sp| sp.id.clone()).collect::<Vec<_>>();
        assert_eq!(ticker_text(&s), vec!["status:connecting"]);
        assert!(!s.has_content());

        let seg = TickerSegment { id: "league:nfl".into(), parts: vec![Part::text(vec![Span::primary("NFL")])] };
        let content = Content { ticker: vec![seg.clone()], crawl: vec![], status: FeedStatus::default() };
        s.apply_feed_event(FeedEvent::Connected);
        s.apply_feed_event(FeedEvent::Message(ServerMsg::Content(content)));
        s.update(0.016, now);
        assert!(s.has_content());
        assert_eq!(ticker_text(&s), vec!["league:nfl"]);
        assert_eq!(segment_text(&seg), "NFL");

        s.apply_feed_event(FeedEvent::Disconnected("server restarted".into()));
        s.update(0.016, now);
        assert_eq!(ticker_text(&s), vec!["league:nfl"], "last content stays up");
    }
}
