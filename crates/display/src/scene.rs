//! Everything on screen, independent of the GPU: layout, the three LED
//! panels (ticker, crawl, placeholder clock) and the mock data driving them.

use chrono::{DateTime, FixedOffset, Utc};
use tickadee_core::alert::Alert;
use tickadee_core::color::led_team_color;
use tickadee_core::config::DisplayConfig;
use tickadee_core::layout::{LedGrid, Rect, ScreenLayout};
use tickadee_core::sports::ticker::{FormatOptions, crawl_segments, ticker_segments};
use tickadee_core::ticker::{Align, Palette, Part, Rasterizer, Span, TickerSegment, Tint};

use crate::band::Band;
use crate::mock::MockFeed;

/// Clock panel width in LEDs; fixed so the dot size never changes.
const CLOCK_COLS: u32 = 112;

#[derive(Debug)]
pub struct Panel {
    pub band: Band,
    pub grid: LedGrid,
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
}

const TICKER: usize = 0;
const CLOCK: usize = 1;
const CRAWL: usize = 2;

impl Scene {
    pub fn new(
        config: DisplayConfig,
        width: u32,
        height: u32,
        now: DateTime<Utc>,
        tz: FixedOffset,
        seed: u64,
        max_strip_width: u32,
    ) -> Self {
        let mut scene = Scene {
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
            },
            {
                let mut rast = Rasterizer::new(c.ticker_rows, palette);
                rast.separator = None;
                let area = inset(l.widgets, 0.72, 0.5);
                Panel {
                    band: Band::new(rast, CLOCK_COLS, 0.0, false, max),
                    grid: LedGrid::fit_centered(area, CLOCK_COLS, c.ticker_rows),
                }
            },
        ];
        if let Some(crawl) = l.crawl {
            panels.push(Panel {
                band: Band::new(Rasterizer::new(crawl.rows, palette), crawl.cols, f64::from(c.crawl_speed), true, max),
                grid: crawl,
            });
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
    }

    /// Advances time by `dt` seconds; `now` is the wall clock.
    pub fn update(&mut self, dt: f64, now: DateTime<Utc>) {
        self.time += dt;
        let update = self.feed.advance(dt, now);
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

    fn scene(w: u32, h: u32) -> Scene {
        let now = Utc.with_ymd_and_hms(2026, 9, 27, 16, 0, 0).unwrap();
        Scene::new(DisplayConfig::default(), w, h, now, FixedOffset::west_opt(4 * 3600).unwrap(), 1, 200_000)
    }

    #[test]
    fn builds_three_panels_with_content() {
        let s = scene(1920, 1080);
        assert_eq!(s.panels.len(), 3);
        for p in &s.panels {
            assert!(p.band.strip.bitmap.data.chunks(4).any(|px| px[3] != 0), "panel has lit LEDs");
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
