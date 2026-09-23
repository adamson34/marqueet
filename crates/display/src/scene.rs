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
use marqueet_core::ticker::{LedBitmap, Palette, Part, RasterStyle, Rasterizer, Span, TickerSegment, Tint};

use crate::band::Band;
use crate::feed::{FeedEvent, LiveFeed};
use crate::header::{self, HeaderState};
use crate::mock::MockFeed;
use crate::takeover;
use crate::ui::{Canvas, Fonts};
use crate::widgets;
use marqueet_core::protocol::{Content, ServerMsg};
use marqueet_core::sports::HomeAway;
use marqueet_core::widgets::{WidgetView, default_views};

/// How long the welcome logo shows at startup, and how long its dots take to
/// sweep on.
pub const WELCOME_SECS: f64 = 4.0;
const WELCOME_REVEAL_SECS: f64 = 0.9;

#[derive(Debug)]
pub struct Panel {
    pub band: Band,
    pub grid: LedGrid,
    pub visible: bool,
    /// Drawn as square LED blocks over the takeover background.
    pub overlay: bool,
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
    takeovers: takeover::Queue,
    /// Panels before this index are the base layout; after it, the active
    /// takeover's text lines.
    base_panels: usize,
    takeover_view: Option<(takeover::Layout, takeover::Palette)>,
    /// CPU-drawn UI layers (header now; widgets later), composited over the LED panels.
    pub ui: Vec<UiLayer>,
    fonts: Fonts,
    header: Option<HeaderState>,
    /// Index of the widget-area layer in `ui`.
    widgets_layer: usize,
    /// Views last drawn into the widget layer.
    widget_views: Option<Vec<WidgetView>>,
}

/// A CPU canvas drawn at `rect`; `dirty` means it needs re-uploading.
#[derive(Debug)]
pub struct UiLayer {
    pub rect: Rect,
    pub canvas: Canvas,
    pub dirty: bool,
    pub opacity: f32,
}

const HEADER_LAYER: usize = 0;

/// What the renderer needs to draw the takeover background.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeoverView {
    pub area: Rect,
    pub palette: takeover::Palette,
    pub opacity: f32,
    /// (rect, corner radius px, fill) for the score box and note pill.
    pub boxes: Vec<(Rect, f32, marqueet_core::Rgb)>,
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
const WELCOME: usize = 1;
const CRAWL: usize = 2;
/// Widgets fade in over this long once the welcome logo is done.
const WIDGETS_FADE_SECS: f64 = 0.5;

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
            takeovers: takeover::Queue::default(),
            base_panels: 0,
            takeover_view: None,
            ui: Vec::new(),
            fonts: Fonts::new(),
            header: None,
            widgets_layer: 0,
            widget_views: None,
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
                overlay: false,
            },
            {
                let logo = welcome_bitmap(palette);
                let (cols, rows) = (logo.width + 10, logo.height + 2);
                let mut band = Band::new(Rasterizer::new(rows, palette), cols, 0.0, false, max);
                band.set_bitmap(LedBitmap::new(logo.width, rows));
                Panel {
                    band,
                    grid: LedGrid::fit_centered(inset(l.widgets, 0.8, 0.62), cols, rows),
                    visible: true,
                    overlay: false,
                }
            },
        ];
        if let Some(crawl) = l.crawl {
            panels.push(Panel {
                band: Band::new(Rasterizer::new(crawl.rows, palette), crawl.cols, f64::from(c.crawl_speed), true, max),
                grid: crawl,
                visible: true,
                overlay: false,
            });
        }
        self.panels = panels;
        self.base_panels = self.panels.len();
        let layer = |r: Rect| UiLayer { rect: r, canvas: Canvas::new(r.w, r.h), dirty: true, opacity: 1.0 };
        self.ui = l.header.map(layer).into_iter().collect();
        self.widgets_layer = self.ui.len();
        self.ui.push(UiLayer { opacity: 0.0, ..layer(l.widgets) });
        self.header = None;
        self.widget_views = None;
        if let Feed::Live { dirty, .. } = &mut self.feed {
            *dirty = true;
        }
        self.rebuild_takeover_panels();
        self.refresh_content(now);
    }

    /// Replaces the takeover text panels with the active takeover's (or none).
    fn rebuild_takeover_panels(&mut self) {
        self.panels.truncate(self.base_panels);
        self.takeover_view = None;
        let Some(active) = self.takeovers.active() else { return };
        let t = &active.takeover;
        let layout = takeover::layout(self.layout.widgets, t);
        let primary = active.alert.colors.map_or(self.config.led_color, |(p, _)| p);
        let palette = takeover::Palette::for_team(primary);
        let grids = [Some(layout.kicker), Some(layout.headline), layout.play, Some(layout.score), layout.note];
        let lines = takeover::segments(t);
        for (grid, (_, seg)) in grids.into_iter().flatten().zip(lines) {
            let mut rast = Rasterizer::new(grid.rows, Palette::new(self.config.led_color));
            rast.separator = None;
            let mut band = Band::new(rast, grid.cols, 0.0, false, self.max_strip_width);
            band.set_segments(vec![seg]);
            self.panels.push(Panel { band, grid, visible: true, overlay: true });
        }
        self.takeover_view = Some((layout, palette));
    }

    /// The active takeover's background, if any.
    pub fn takeover_view(&self) -> Option<TakeoverView> {
        let (layout, palette) = self.takeover_view.as_ref()?;
        let mut boxes = vec![(layout.score_box, layout.score.pitch as f32 * 1.5, palette.box_fill)];
        if let Some(pill) = layout.note_pill {
            boxes.push((pill, pill.h as f32 / 2.0, takeover::CREAM));
        }
        Some(TakeoverView { area: layout.area, palette: *palette, opacity: self.takeover_opacity(), boxes })
    }

    pub fn takeover_opacity(&self) -> f32 {
        self.takeovers.opacity(self.time)
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
        self.refresh_header(now);
        self.refresh_widgets(now);
        self.apply_visibility();
        if self.time < WELCOME_SECS {
            let logo = welcome_bitmap(Palette::new(self.config.led_color));
            let t = (self.time / WELCOME_REVEAL_SECS).min(1.0);
            let mut framed = LedBitmap::new(logo.width, logo.height + 2);
            framed.blit(&logo.reveal((t * f64::from(logo.width)).ceil() as u32), 0, 1);
            self.panels[WELCOME].band.set_bitmap(framed);
        }
    }

    /// Welcome logo first, then the widgets fade in; a takeover covers both.
    fn apply_visibility(&mut self) {
        let welcome = self.time < WELCOME_SECS;
        let takeover = self.takeovers.active().is_some();
        self.panels[WELCOME].visible = welcome && !takeover;
        let fade = ((self.time - WELCOME_SECS) / WIDGETS_FADE_SECS).clamp(0.0, 1.0) as f32;
        if let Some(layer) = self.ui.get_mut(self.widgets_layer) {
            layer.opacity = fade;
        }
    }

    /// Opacity of the widget area (0 during the welcome logo).
    #[cfg(test)]
    pub fn widgets_opacity(&self) -> f32 {
        self.ui.get(self.widgets_layer).map_or(0.0, |l| l.opacity)
    }

    fn refresh_widgets(&mut self, now: DateTime<Utc>) {
        let views = match &self.feed {
            Feed::Mock(feed) => default_views(&feed.games, &[], self.tz, now),
            Feed::Live { content, .. } => content.as_ref().map(|c| c.widgets.clone()).unwrap_or_default(),
        };
        if self.widget_views.as_ref() == Some(&views) {
            return;
        }
        if let Some(layer) = self.ui.get_mut(self.widgets_layer) {
            widgets::draw(&mut layer.canvas, &mut self.fonts, &views);
            layer.dirty = true;
        }
        self.widget_views = Some(views);
    }

    /// Games in progress right now, from whichever feed is active.
    pub fn live_games(&self) -> u32 {
        match &self.feed {
            Feed::Mock(feed) => feed.games.iter().filter(|g| g.status.is_live()).count() as u32,
            Feed::Live { content, .. } => content.as_ref().map_or(0, |c| c.status.live_games),
        }
    }

    fn refresh_header(&mut self, now: DateTime<Utc>) {
        let state = HeaderState {
            live_games: self.live_games(),
            label: "ALL LEAGUES".into(),
            time: now.with_timezone(&self.tz).format("%-I:%M %p").to_string(),
        };
        let Some(layer) = self.ui.get_mut(HEADER_LAYER) else { return };
        if self.header.as_ref() != Some(&state) {
            header::draw(&mut layer.canvas, &mut self.fonts, &state);
            layer.dirty = true;
            self.header = Some(state);
        }
    }

    /// Scores `points` for one side of a mock game and flashes it on the
    /// ticker, exactly as a live scoring alert would (headless `--score`).
    pub fn score(&mut self, game_id: &str, side: HomeAway, points: u16) -> bool {
        let Feed::Mock(feed) = &mut self.feed else { return false };
        if !feed.games.iter().any(|g| g.id.0 == game_id) {
            return false;
        }
        let now = feed.now;
        let alerts = feed.score(game_id, side, points, now);
        self.refresh_content(now);
        self.handle_alerts(&alerts);
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
        self.handle_alerts(&alerts);
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
            FeedEvent::Message(ServerMsg::Alert(a)) => return Some(*a),
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

    /// Flashes each alert's ticker segment and queues takeovers.
    fn handle_alerts(&mut self, alerts: &[Alert]) {
        for alert in alerts {
            self.apply_alert(alert);
            self.takeovers.push(alert.clone(), self.time);
        }
        if self.takeovers.update(self.time) {
            self.rebuild_takeover_panels();
            self.apply_visibility();
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
    ScreenLayout::compute(width, height, c)
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
        assert_eq!(s.panels.len(), 3, "ticker, welcome, crawl");
        s.update(WELCOME_REVEAL_SECS, Utc::now());
        for p in &s.panels {
            assert!(p.band.strip.bitmap.data.chunks(4).any(|px| px[3] != 0), "panel has lit LEDs");
        }
    }

    #[test]
    fn welcome_logo_sweeps_on_then_hands_over_to_the_clock() {
        let mut s = scene(1920, 1080);
        let lit = |s: &Scene| s.panels[WELCOME].band.strip.bitmap.data.chunks(4).filter(|px| px[3] != 0).count();
        assert!(s.panels[WELCOME].visible && s.widgets_opacity() == 0.0);
        s.update(WELCOME_REVEAL_SECS / 3.0, Utc::now());
        let partial = lit(&s);
        s.update(WELCOME_REVEAL_SECS, Utc::now());
        assert!(lit(&s) > partial && partial > 0, "dots sweep on");
        s.update(WELCOME_SECS, Utc::now());
        assert!(!s.panels[WELCOME].visible);
        s.update(WIDGETS_FADE_SECS, Utc::now());
        assert_eq!(s.widgets_opacity(), 1.0, "widgets faded in");
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
        let content =
            Content { ticker: vec![seg.clone()], crawl: vec![], status: FeedStatus::default(), widgets: vec![] };
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

    /// Past the welcome, with no takeover showing and the between-takeover
    /// gap elapsed (the mock feed scores on its own every few seconds).
    fn settle(s: &mut Scene) {
        s.update(WELCOME_SECS + 0.1, Utc::now());
        loop {
            while s.takeovers.active().is_some() {
                s.update(0.5, Utc::now());
            }
            s.update(1.0, Utc::now());
            if s.takeovers.active().is_none() {
                return;
            }
        }
    }

    #[test]
    fn touchdown_takes_over_the_widget_area_then_hands_back() {
        let mut s = scene(1920, 1080);
        settle(&mut s);
        let base = s.panels.len();
        assert!(s.score("mock:nfl:1", HomeAway::Home, 7));
        let active = s.takeovers.active().expect("touchdown takes over");
        assert_eq!(active.takeover.headline, "TOUCHDOWN");
        assert_eq!(s.panels.len(), base + 4, "kicker, headline, play, score");
        assert!(s.panels[base..].iter().all(|p| p.overlay && p.visible));
        assert!(!s.panels[WELCOME].visible);
        let view = s.takeover_view().unwrap();
        assert_eq!(view.area, s.layout.widgets);
        assert_eq!(view.opacity, 0.0, "fades in");
        s.update(1.0, Utc::now());
        assert_eq!(s.takeover_view().unwrap().opacity, 1.0);
        for _ in 0..30 {
            s.update(0.5, Utc::now());
            if s.takeovers.active().is_none() {
                break;
            }
        }
        assert!(s.takeovers.active().is_none(), "ends after ~10 s");
        assert_eq!(s.panels.len(), base);
        assert_eq!(s.widgets_opacity(), 1.0);
    }

    #[test]
    fn field_goal_only_flashes() {
        let mut s = scene(1920, 1080);
        settle(&mut s);
        assert!(s.score("mock:nfl:1", HomeAway::Away, 3));
        assert!(s.takeovers.active().is_none());
        assert!(s.panels[TICKER].band.is_flashing("mock:nfl:1"));
    }
}
