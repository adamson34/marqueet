//! Everything on screen, independent of the GPU: layout, the LED panels
//! (ticker, crawl, welcome logo, placeholder clock) and the mock data
//! driving them.

use chrono::{DateTime, FixedOffset, Utc};
use tickadee_core::alert::Alert;
use tickadee_core::color::led_team_color;
use tickadee_core::config::DisplayConfig;
use tickadee_core::layout::{LedGrid, Rect, ScreenLayout};
use tickadee_core::logo::DotMark;
use tickadee_core::sports::ticker::{FormatOptions, crawl_segments, ticker_segments};
use tickadee_core::ticker::{Align, LedBitmap, Palette, Part, RasterStyle, Rasterizer, Span, TickerSegment, Tint};

use crate::band::Band;
use crate::mock::MockFeed;
use crate::mockup;
use tickadee_core::sports::HomeAway;

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
    feed: MockFeed,
    tz: FixedOffset,
    /// Seconds since the scene started.
    pub time: f64,
    max_strip_width: u32,
    /// Concept mockup of planned widgets (`--mockup`); see `mockup.rs`.
    mockup: Option<MockupPanels>,
    mockup_takeover_fired: bool,
}

/// Panel indices used by the concept mockup.
#[derive(Debug, Default)]
struct MockupPanels {
    left: Vec<usize>,
    right: Vec<usize>,
    title: usize,
    lines: Vec<usize>,
}

const TICKER: usize = 0;
const CLOCK: usize = 1;
const WELCOME: usize = 2;
const CRAWL: usize = 3;

/// How a scene starts: clock, time zone, mock data and GPU limits.
#[derive(Debug, Clone, Copy)]
pub struct SceneSetup {
    pub now: DateTime<Utc>,
    pub tz: FixedOffset,
    /// Seed for the mock feed.
    pub seed: u64,
    /// Longest strip the GPU can hold (see `Renderer::max_strip_width`).
    pub max_strip_width: u32,
    /// Show the concept mockup of planned widgets.
    pub mockup: bool,
}

impl Scene {
    pub fn new(config: DisplayConfig, width: u32, height: u32, setup: SceneSetup) -> Self {
        let SceneSetup { now, tz, seed, max_strip_width, mockup } = setup;
        let mut scene = Scene {
            mockup: mockup.then(MockupPanels::default),
            mockup_takeover_fired: false,
            layout: compute_layout(&config, width, height),
            config,
            panels: Vec::new(),
            feed: MockFeed::new(now, seed),
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
        if self.mockup.is_some() {
            let layout = mockup::layout(l.widgets);
            let mut add = |grid: LedGrid, left: bool| {
                let mut rast = Rasterizer::new(grid.rows, palette);
                rast.separator = None;
                let mut band = Band::new(rast, grid.cols, 0.0, false, max);
                band.align_left = left;
                panels.push(Panel { band, grid, visible: false });
                panels.len() - 1
            };
            let m = MockupPanels {
                left: layout.left.iter().map(|g| add(*g, true)).collect(),
                right: layout.right.iter().map(|g| add(*g, true)).collect(),
                title: add(layout.takeover_title, false),
                lines: layout.takeover_lines.iter().map(|g| add(*g, false)).collect(),
            };
            self.mockup = Some(m);
        }
        self.panels = panels;
        self.refresh_content(now);
    }

    fn format_options(&self, now: DateTime<Utc>) -> FormatOptions {
        FormatOptions { tz: self.tz, now }
    }

    fn refresh_content(&mut self, now: DateTime<Utc>) {
        let opts = self.format_options(now);
        let games = &self.feed.games;
        let ticker = ticker_segments(games, &opts);
        let crawl = crawl_segments(games, &opts);
        self.panels[TICKER].band.set_segments(ticker);
        if let Some(p) = self.panels.get_mut(CRAWL) {
            p.band.set_segments(crawl);
        }
        let clock = clock_segment(now.with_timezone(&self.tz));
        self.panels[CLOCK].band.set_segments(vec![clock]);

        if self.mockup.is_some() {
            self.panels[WELCOME].visible = false;
            self.panels[CLOCK].visible = false;
            self.refresh_mockup();
            return;
        }
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

    fn refresh_mockup(&mut self) {
        let Some(m) = &self.mockup else { return };
        let Some(game) = self.feed.games.iter().find(|g| g.id.0 == mockup::FEATURED_GAME).cloned() else {
            return;
        };
        let takeover = (mockup::TAKEOVER_AT..mockup::TAKEOVER_AT + mockup::TAKEOVER_SECS).contains(&self.time);
        let (title, lines) = mockup::takeover(&game);
        let content = [
            (m.left.clone(), mockup::game_of_the_day(&game), !takeover),
            (m.right.clone(), mockup::standings_and_fantasy(), !takeover),
            (vec![m.title], vec![title], takeover),
            (m.lines.clone(), lines, takeover),
        ];
        for (indices, segments, visible) in content {
            for (i, seg) in indices.into_iter().zip(segments) {
                let empty = seg.parts.is_empty();
                self.panels[i].visible = visible && !empty;
                self.panels[i].band.set_segments(if empty { Vec::new() } else { vec![seg] });
            }
        }
    }

    /// At the mockup's takeover moment: score the featured game and flash it.
    fn fire_mockup_takeover(&mut self) {
        let Some(m) = &self.mockup else { return };
        if self.mockup_takeover_fired || self.time < mockup::TAKEOVER_AT {
            return;
        }
        self.mockup_takeover_fired = true;
        let title = m.title;
        self.feed.add_points(mockup::FEATURED_GAME, HomeAway::Home, 7);
        let color = self
            .feed
            .games
            .iter()
            .find(|g| g.id.0 == mockup::FEATURED_GAME)
            .map(|g| led_team_color(g.home.team.colors.primary, g.home.team.colors.secondary))
            .unwrap_or(self.config.led_color);
        self.panels[TICKER].band.flash(mockup::FEATURED_GAME, color, self.time);
        self.panels[title].band.flash("takeover:title", color, self.time);
    }

    /// Advances time by `dt` seconds; `now` is the wall clock.
    pub fn update(&mut self, dt: f64, now: DateTime<Utc>) {
        self.time += dt;
        let update = self.feed.advance(dt, now);
        self.fire_mockup_takeover();
        // Clock text changes once a minute; set_segments skips no-op updates.
        self.refresh_content(now);
        for alert in &update.alerts {
            self.apply_alert(alert);
        }
        for p in &mut self.panels {
            p.band.update(dt, self.time);
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

/// The chickadee mark followed by the TICKADEE wordmark, 16 LEDs tall.
fn welcome_bitmap(palette: Palette) -> LedBitmap {
    // Cap, bib and wing in the LED color; the body in a dim buff, like the
    // bird's pale belly, so the two read apart on the panel.
    let buff = tickadee_core::Rgb::new(255, 226, 180).scale(0.42);
    let mark = DotMark::mark().to_led_bitmap(palette.primary, buff);
    let mut rast = Rasterizer::new(mark.height, palette);
    rast.separator = None;
    let word = rast.render_segment(
        &TickerSegment { id: "welcome".into(), parts: vec![Part::text(vec![Span::primary("TICKADEE")])] },
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

    fn setup(now: DateTime<Utc>, tz: FixedOffset, mockup: bool) -> SceneSetup {
        SceneSetup { now, tz, seed: 1, max_strip_width: 200_000, mockup }
    }

    fn scene(w: u32, h: u32) -> Scene {
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        Scene::new(DisplayConfig::default(), w, h, setup(now, FixedOffset::west_opt(4 * 3600).unwrap(), false))
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
    fn mockup_shows_widgets_then_a_takeover() {
        let now = Utc::now();
        let mut s =
            Scene::new(DisplayConfig::default(), 1920, 1080, setup(now, FixedOffset::east_opt(0).unwrap(), true));
        let m = s.mockup.as_ref().map(|m| (m.left[0], m.title)).unwrap();
        s.update(0.1, now);
        assert!(s.panels[m.0].visible && !s.panels[m.1].visible);
        assert!(!s.panels[WELCOME].visible && !s.panels[CLOCK].visible);
        while s.time < mockup::TAKEOVER_AT + 0.5 {
            s.update(0.1, now);
        }
        assert!(!s.panels[m.0].visible && s.panels[m.1].visible, "takeover replaces widgets");
        while s.time < mockup::TAKEOVER_AT + mockup::TAKEOVER_SECS + 0.2 {
            s.update(0.1, now);
        }
        assert!(s.panels[m.0].visible && !s.panels[m.1].visible, "and hands back");
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
        let text = tickadee_core::sports::ticker::segment_text(&clock_segment(t));
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
}
