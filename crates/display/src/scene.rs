//! Everything on screen, independent of the GPU: layout, the LED panels
//! (ticker, crawl, welcome logo, placeholder clock) and the mock data
//! driving them.

use std::sync::Arc;

use chrono::{DateTime, FixedOffset, Utc};
use marqueet_core::alert::Alert;
use marqueet_core::color::led_team_color;
use marqueet_core::config::DisplayConfig;
use marqueet_core::layout::{LedGrid, Rect, ScreenLayout};
use marqueet_core::logo::DotMark;
use marqueet_core::sports::ticker::{FormatOptions, crawl_label, crawl_segments, ticker_segments};
use marqueet_core::ticker::{LedBitmap, Logos, Palette, Part, RasterStyle, Rasterizer, Span, TickerSegment, Tint};

use crate::band::Band;
use crate::crawl;
use crate::feed::{FeedEvent, LiveFeed};
use crate::gpu;
use crate::header::{self, HeaderState};
use crate::mock::MockFeed;
use crate::takeover;
use crate::ui::{Canvas, Fonts};
use crate::widgets;
use marqueet_core::protocol::{Content, DisplayState, ServerMsg, SetupInfo};
use marqueet_core::settings::{SpotlightSettings, WidgetSlot};
use marqueet_core::sports::GameId;
use marqueet_core::sports::HomeAway;
use marqueet_core::sports::fixtures::mock_standings;
use marqueet_core::weather::mock_weather;
use marqueet_core::widgets::{WidgetData, WidgetView, build_views};

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
    /// With `--mock --spotlight`: the game to spotlight.
    mock_spotlight: Option<SpotlightSettings>,
    pub config: DisplayConfig,
    pub layout: ScreenLayout,
    pub panels: Vec<Panel>,
    feed: Feed,
    /// Time zone for the clock and start times: the server's setting when it
    /// sends one, else `device_tz`.
    tz: FixedOffset,
    device_tz: FixedOffset,
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
    /// Indices of the crawl strip and tag layers in `ui`.
    crawl_layers: Option<(usize, usize)>,
    crawl: Option<CrawlState>,
    /// Views last drawn into the widget layer.
    widget_views: Option<Vec<WidgetView>>,
    /// First boot: the setup screen replaces the widgets.
    setup: Option<SetupInfo>,
    /// The setup screen currently drawn, if any.
    drawn_setup: Option<SetupInfo>,
    /// Quiet hours: draw nothing.
    pub screen_off: bool,
    /// Display settings from the server, applied on the next update.
    pending_display: Option<DisplayState>,
    /// Team logos people added (from the server), by key.
    logos: Arc<Logos>,
    /// A new set of logos, applied on the next update.
    pending_logos: Option<Logos>,
}

/// A CPU canvas drawn at `rect`; `dirty` means it needs re-uploading.
#[derive(Debug)]
pub struct UiLayer {
    pub rect: Rect,
    pub canvas: Canvas,
    pub dirty: bool,
    pub opacity: f32,
    /// Set for a strip that scrolls through `rect`, wrapping (the crawl).
    pub scroll: Option<Scroll>,
}

/// Scroll state of a strip layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scroll {
    /// Pixels per second.
    pub speed: f64,
    /// Pixels scrolled so far.
    pub pos: f64,
}

impl Scroll {
    /// Whole-pixel offset into a strip `width` px wide (crisp text).
    pub fn offset(&self, width: u32) -> f32 {
        (self.pos.floor() % f64::from(width.max(1))) as f32
    }
}

/// What the crawl shows; redrawn only when this changes.
#[derive(Clone, Debug, PartialEq)]
struct CrawlState {
    label: Option<String>,
    segments: Vec<TickerSegment>,
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
    /// Built-in demo data (no server needed) with these widget slots.
    Mock {
        seed: u64,
        widgets: Vec<WidgetSlot>,
        /// Spotlight the demo's featured football game.
        spotlight: bool,
    },
    /// `marqueet-server` at this WebSocket URL.
    Live { url: String },
}

#[derive(Debug)]
enum Feed {
    /// Demo games and the widget slots to show them in.
    Mock(MockFeed, Vec<WidgetSlot>),
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
        let mock_spotlight = matches!(source, FeedSource::Mock { spotlight: true, .. })
            .then(|| SpotlightSettings { auto: false, game: Some(GameId("mock:nfl:1".into())) });
        let feed = match source {
            FeedSource::Mock { seed, widgets, .. } => Feed::Mock(MockFeed::new(now, seed), widgets),
            FeedSource::Live { url } => {
                log::info!("connecting to {url}");
                Feed::Live { client: LiveFeed::connect(&url), url, content: None, connected: false, dirty: true }
            }
        };
        let mut scene = Scene {
            mock_spotlight,
            layout: compute_layout(&config, width, height),
            config,
            panels: Vec::new(),
            feed,
            tz,
            device_tz: tz,
            time: 0.0,
            max_strip_width,
            takeovers: takeover::Queue::default(),
            base_panels: 0,
            takeover_view: None,
            ui: Vec::new(),
            fonts: Fonts::new(),
            header: None,
            widgets_layer: 0,
            crawl_layers: None,
            crawl: None,
            widget_views: None,
            setup: None,
            drawn_setup: None,
            screen_off: false,
            pending_display: None,
            logos: Arc::default(),
            pending_logos: None,
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
        let panels = vec![
            Panel {
                band: Band::new(
                    Rasterizer::new(l.ticker.rows, palette).with_logos(self.logos.clone()),
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
        self.panels = panels;
        self.base_panels = self.panels.len();
        let layer =
            |r: Rect| UiLayer { rect: r, canvas: Canvas::new(r.w, r.h), dirty: true, opacity: 1.0, scroll: None };
        self.ui = l.header.map(layer).into_iter().collect();
        self.crawl_layers = None;
        self.crawl = None;
        if let Some(band) = l.crawl {
            let speed = f64::from(c.crawl_speed) * f64::from(band.h) / 10.0;
            let strip = UiLayer { scroll: Some(Scroll { speed, pos: 0.0 }), ..layer(band) };
            let tag_w = crawl::tag_width(&self.fonts, &c.theme, band.h).min(band.w);
            let tag = UiLayer { opacity: 0.0, ..layer(Rect { w: tag_w, ..band }) };
            self.crawl_layers = Some((self.ui.len(), self.ui.len() + 1));
            self.ui.extend([strip, tag]);
        }
        self.widgets_layer = self.ui.len();
        self.ui.push(UiLayer { opacity: 0.0, ..layer(l.widgets) });
        self.header = None;
        self.widget_views = None;
        // The new widget layer is blank: redraw the setup card too.
        self.drawn_setup = None;
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
        let palette = takeover::Palette::new(&self.config.theme, active.alert.colors, self.config.led_color);
        let grids = [Some(layout.kicker), Some(layout.headline), layout.play, layout.score, layout.note];
        let lines = takeover::segments(t, &palette);
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
        let mut boxes = Vec::new();
        if let (Some(b), Some(score)) = (layout.score_box, layout.score) {
            boxes.push((b, score.pitch as f32 * 1.5, palette.box_fill));
        }
        if let Some(pill) = layout.note_pill {
            boxes.push((pill, pill.h as f32 / 2.0, palette.pill));
        }
        Some(TakeoverView { area: layout.area, palette: *palette, opacity: self.takeover_opacity(), boxes })
    }

    pub fn takeover_opacity(&self) -> f32 {
        self.takeovers.opacity(self.time)
    }

    fn format_options(&self, now: DateTime<Utc>) -> FormatOptions {
        FormatOptions { tz: self.tz, now }
    }

    fn set_ticker_and_crawl(&mut self, ticker: Vec<TickerSegment>, crawl: Vec<TickerSegment>, label: Option<String>) {
        self.panels[TICKER].band.set_segments(ticker);
        let state = CrawlState { label, segments: crawl };
        let Some((strip, tag)) = self.crawl_layers else { return };
        if self.crawl.as_ref() == Some(&state) {
            return;
        }
        let h = self.ui[strip].rect.h;
        let canvas = crawl::draw_strip(&mut self.fonts, &self.config.theme, &state.segments, h, gpu::ui_strip_max(h));
        let layer = &mut self.ui[strip];
        layer.canvas = canvas;
        layer.dirty = true;
        let tag_layer = &mut self.ui[tag];
        match &state.label {
            Some(label) => {
                crawl::draw_tag(&mut tag_layer.canvas, &mut self.fonts, &self.config.theme, label);
                tag_layer.opacity = 1.0;
                tag_layer.dirty = true;
            }
            None => tag_layer.opacity = 0.0,
        }
        self.crawl = Some(state);
    }

    fn refresh_content(&mut self, now: DateTime<Utc>) {
        let opts = self.format_options(now);
        match &mut self.feed {
            Feed::Mock(feed, _) => {
                let games = &feed.games;
                let mut ticker = ticker_segments(games, &opts);
                ticker.insert(0, marqueet_core::fantasy::ticker_segment(&marqueet_core::fantasy::mock_matchup(now)));
                ticker.insert(0, marqueet_core::weather::ticker_segment(&mock_weather(now)));
                let (crawl, label) = (crawl_segments(games, &opts), crawl_label(games, &opts));
                self.set_ticker_and_crawl(ticker, crawl, label);
            }
            Feed::Live { content, url, dirty, .. } => {
                // Server content arrives ready to render; apply it only when
                // it changed rather than cloning it every frame.
                if std::mem::take(dirty) {
                    let (ticker, crawl, label) = match content {
                        Some(c) => (c.ticker.clone(), c.crawl.clone(), c.crawl_label.clone()),
                        None => (
                            vec![notice("status:connecting", "CONNECTING TO SERVER")],
                            vec![notice("status:url", url)],
                            None,
                        ),
                    };
                    self.set_ticker_and_crawl(ticker, crawl, label);
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
        if let Some(info) = &self.setup {
            if self.drawn_setup.as_ref() != Some(info)
                && let Some(layer) = self.ui.get_mut(self.widgets_layer)
            {
                crate::setup::draw(&mut layer.canvas, &mut self.fonts, info, &self.config.theme);
                layer.dirty = true;
                self.drawn_setup = Some(info.clone());
                self.widget_views = None;
            }
            return;
        }
        self.drawn_setup = None;
        let views = match &self.feed {
            Feed::Mock(feed, kinds) => {
                let (standings, weather) = (mock_standings(now), mock_weather(now));
                let fantasy = [marqueet_core::fantasy::mock_matchup(now)];
                let data = WidgetData {
                    games: &feed.games,
                    standings: &standings,
                    weather: Some(&weather),
                    favorites: &[],
                    fantasy: &fantasy,
                    art: None,
                    spotlight: self.mock_spotlight.as_ref(),
                };
                build_views(kinds, &data, self.tz, now)
            }
            Feed::Live { content, .. } => content.as_ref().map(|c| c.widgets.clone()).unwrap_or_default(),
        };
        if self.widget_views.as_ref() == Some(&views) {
            return;
        }
        if let Some(layer) = self.ui.get_mut(self.widgets_layer) {
            widgets::draw(
                &mut layer.canvas,
                &mut self.fonts,
                &views,
                self.config.widget_layout,
                &self.config.theme,
                &self.logos,
            );
            layer.dirty = true;
        }
        self.widget_views = Some(views);
    }

    /// Games in progress right now, from whichever feed is active.
    pub fn live_games(&self) -> u32 {
        match &self.feed {
            Feed::Mock(feed, _) => feed.games.iter().filter(|g| g.status.is_live()).count() as u32,
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
            header::draw(&mut layer.canvas, &mut self.fonts, &state, &self.config.theme);
            layer.dirty = true;
            self.header = Some(state);
        }
    }

    /// Scores `points` for one side of a mock game and flashes it on the
    /// ticker, exactly as a live scoring alert would (headless `--score`).
    pub fn score(&mut self, game_id: &str, side: HomeAway, points: u16) -> bool {
        let Feed::Mock(feed, _) = &mut self.feed else { return false };
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
            Feed::Mock(feed, _) => feed.advance(dt, now).alerts,
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
        if let Some(display) = self.pending_display.take() {
            self.apply_display(display, now);
        }
        if let Some(logos) = self.pending_logos.take()
            && *self.logos != logos
        {
            log::info!("{} team logo{}", logos.len(), if logos.len() == 1 { "" } else { "s" });
            self.logos = Arc::new(logos);
            self.rebuild_panels(now);
        }
        // Clock text changes once a minute; set_segments skips no-op updates.
        self.refresh_content(now);
        self.handle_alerts(&alerts);
        for p in &mut self.panels {
            p.band.update(dt, self.time);
        }
        for s in self.ui.iter_mut().filter_map(|l| l.scroll.as_mut()) {
            s.pos += s.speed * dt;
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
            FeedEvent::Message(ServerMsg::Display(d)) => self.pending_display = Some(*d),
            FeedEvent::Message(ServerMsg::Logos { logos, replace }) => {
                // Only well-formed images; a bad one is dropped, not drawn.
                let valid = logos.into_iter().filter(|(_, image)| image.is_valid());
                let mut next = if replace {
                    Logos::new()
                } else {
                    self.pending_logos.take().unwrap_or_else(|| (*self.logos).clone())
                };
                next.extend(valid);
                self.pending_logos = Some(next);
            }
        }
        None
    }

    /// Applies display settings from the server: a new look rebuilds the
    /// layout; quiet hours blank the screen.
    fn apply_display(&mut self, display: DisplayState, now: DateTime<Utc>) {
        let config = display.config.sanitized();
        if config != self.config {
            log::info!("display settings updated");
            self.config = config;
            self.layout = compute_layout(&self.config, self.layout.width, self.layout.height);
            self.rebuild_panels(now);
        }
        let tz = display.utc_offset.and_then(FixedOffset::east_opt).unwrap_or(self.device_tz);
        if tz != self.tz {
            log::info!("time zone offset now {tz}");
            self.tz = tz;
        }
        if display.setup != self.setup {
            log::info!(
                "{}",
                if display.setup.is_some() {
                    "first boot: showing the setup screen"
                } else {
                    "set up: showing widgets"
                }
            );
            self.setup = display.setup;
        }
        if display.screen_off != self.screen_off {
            log::info!("quiet hours {}", if display.screen_off { "started: screen off" } else { "ended: screen on" });
            self.screen_off = display.screen_off;
        }
    }

    /// True once live content has arrived (always true for mock data).
    pub fn has_content(&self) -> bool {
        match &self.feed {
            Feed::Mock(..) => true,
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
        SceneSetup {
            now,
            tz,
            source: FeedSource::Mock {
                seed: 1,
                widgets: marqueet_core::settings::Settings::default().widgets,
                spotlight: false,
            },
            max_strip_width: 200_000,
        }
    }

    fn scene(w: u32, h: u32) -> Scene {
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        Scene::new(DisplayConfig::default(), w, h, setup(now, FixedOffset::west_opt(4 * 3600).unwrap()))
    }

    #[test]
    fn builds_panels_with_content() {
        let mut s = scene(1920, 1080);
        assert_eq!(s.panels.len(), 2, "ticker, welcome");
        s.update(WELCOME_REVEAL_SECS, Utc::now());
        for p in &s.panels {
            assert!(p.band.strip.bitmap.data.chunks(4).any(|px| px[3] != 0), "panel has lit LEDs");
        }
    }

    #[test]
    fn crawl_is_a_flat_scrolling_strip_behind_a_tag() {
        let mut s = scene(1920, 1080);
        let (strip, tag) = s.crawl_layers.expect("crawl layers");
        let band = s.layout.crawl.unwrap();
        assert_eq!(s.ui[strip].rect, band);
        assert!(s.ui[strip].canvas.width > 300, "upcoming games drawn");
        assert_eq!(s.ui[tag].rect.x, 0);
        assert_eq!(s.ui[tag].opacity, 1.0, "mock data has upcoming games");
        let ink = s.config.theme.palette.strip_text;
        assert!(s.ui[tag].canvas.pixel(2, 2)[..3] == [ink.r, ink.g, ink.b], "broadcast tag");
        let before = s.ui[strip].scroll.unwrap().pos;
        s.ui[strip].dirty = false;
        s.update(0.5, Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 1).unwrap());
        assert!(s.ui[strip].scroll.unwrap().pos > before, "scrolls");
        assert!(!s.ui[strip].dirty, "scrolling needs no redraw");
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
        let Feed::Mock(feed, _) = &s.feed else { panic!() };
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
    fn setup_card_survives_a_resize() {
        let now = Utc::now();
        let mut s = scene(1920, 1080);
        s.setup = Some(SetupInfo { code: "123456".into(), urls: vec!["http://marqueet.local:7878/setup".into()] });
        s.update(0.1, now);
        let drawn = |s: &Scene| s.ui[s.widgets_layer].canvas.data.chunks(4).any(|p| p[3] != 0);
        assert!(drawn(&s), "setup card drawn");
        s.resize(1366, 768, now);
        s.update(0.1, now);
        assert!(drawn(&s), "setup card redrawn after the screen size changes");
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
        let content = Content {
            ticker: vec![seg.clone()],
            crawl: vec![],
            crawl_label: None,
            status: FeedStatus::default(),
            widgets: vec![],
        };
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
        assert_eq!(s.panels.len(), base + 5, "kicker, headline, play, score, fantasy note");
        assert_eq!(active.takeover.note.as_ref().map(|n| n.0.as_str()), Some("YOUR STARTER"));
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

    #[test]
    fn server_display_settings_restyle_and_blank_the_screen() {
        let now = Utc::now();
        let source = FeedSource::Live { url: "ws://127.0.0.1:9/ws".into() };
        let mut s = Scene::new(
            DisplayConfig::default(),
            1920,
            1080,
            SceneSetup { now, tz: FixedOffset::east_opt(0).unwrap(), source, max_strip_width: 200_000 },
        );
        let green = marqueet_core::Rgb::new(0, 255, 0);
        let config = DisplayConfig { led_color: green, ticker_rows: 21, ..DisplayConfig::default() };
        s.apply_feed_event(FeedEvent::Message(ServerMsg::Display(Box::new(DisplayState {
            config: config.clone(),
            screen_off: true,
            utc_offset: Some(-5 * 3600),
            setup: None,
        }))));
        s.update(0.016, now);
        assert_eq!(s.config, config);
        assert_eq!(s.panels[TICKER].band.rast.palette.primary, green);
        assert_eq!(s.panels[TICKER].grid.rows, 21, "layout rebuilt");
        assert!(s.screen_off);
        assert_eq!(s.tz.local_minus_utc(), -5 * 3600, "server's time zone");
        s.apply_feed_event(FeedEvent::Message(ServerMsg::Display(Box::new(DisplayState {
            config,
            screen_off: false,
            utc_offset: None,
            setup: None,
        }))));
        s.update(0.016, now);
        assert!(!s.screen_off);
        assert_eq!(s.tz.local_minus_utc(), 0, "back to the device's zone");
    }
}
