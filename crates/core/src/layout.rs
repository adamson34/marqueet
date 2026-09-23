//! Splits the screen into the main ticker, the crawl and the widget area,
//! and fits an LED grid into each ticker band.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// An LED grid inside a band. The grid spans the full band width (the last
/// column may be clipped) and is vertically centered, so every dot is square
/// and lands on whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedGrid {
    /// The band the grid lives in (background drawn here).
    pub band: Rect,
    /// Pixel position of the top-left LED cell.
    pub origin: (u32, u32),
    /// LED cell size in pixels (square).
    pub pitch: u32,
    pub cols: u32,
    pub rows: u32,
}

impl LedGrid {
    pub fn fit(band: Rect, rows: u32) -> LedGrid {
        let rows = rows.max(1);
        let pitch = (band.h / rows).max(1);
        let grid_h = pitch * rows;
        let origin_y = band.y + band.h.saturating_sub(grid_h) / 2;
        LedGrid { band, origin: (band.x, origin_y), pitch, cols: band.w.div_ceil(pitch), rows }
    }

    /// Fits a grid of `cols` x `rows` LEDs centered in `area`, as large as fits.
    pub fn fit_centered(area: Rect, cols: u32, rows: u32) -> LedGrid {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let pitch = (area.w / cols).min(area.h / rows).max(1);
        let (w, h) = (pitch * cols, pitch * rows);
        let origin = (area.x + area.w.saturating_sub(w) / 2, area.y + area.h.saturating_sub(h) / 2);
        LedGrid { band: Rect { x: origin.0, y: origin.1, w, h }, origin, pitch, cols, rows }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenLayout {
    pub width: u32,
    pub height: u32,
    pub ticker: LedGrid,
    /// `None` when the crawl is disabled (`crawl_share` = 0).
    pub crawl: Option<LedGrid>,
    pub widgets: Rect,
}

impl ScreenLayout {
    pub fn compute(
        width: u32,
        height: u32,
        ticker_ratio: f32,
        crawl_share: f32,
        ticker_rows: u32,
        crawl_rows: u32,
    ) -> ScreenLayout {
        let ticker_h = ((height as f32 * ticker_ratio).round() as u32).min(height);
        let crawl_h = (ticker_h as f32 * crawl_share).round() as u32;
        let main_h = ticker_h - crawl_h;
        let ticker = LedGrid::fit(Rect { x: 0, y: 0, w: width, h: main_h }, ticker_rows);
        let crawl = (crawl_h > 0).then(|| LedGrid::fit(Rect { x: 0, y: main_h, w: width, h: crawl_h }, crawl_rows));
        let widgets = Rect { x: 0, y: ticker_h, w: width, h: height - ticker_h };
        ScreenLayout { width, height, ticker, crawl, widgets }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DisplayConfig;

    fn default_layout(w: u32, h: u32) -> ScreenLayout {
        let c = DisplayConfig::default();
        ScreenLayout::compute(w, h, c.ticker_ratio, c.crawl_share, c.ticker_rows, c.crawl_rows)
    }

    #[test]
    fn full_hd() {
        let l = default_layout(1920, 1080);
        assert_eq!(l.ticker.band, Rect { x: 0, y: 0, w: 1920, h: 259 });
        assert_eq!(l.ticker.pitch, 13);
        assert_eq!(l.ticker.cols, 148);
        let crawl = l.crawl.unwrap();
        assert_eq!(crawl.band, Rect { x: 0, y: 259, w: 1920, h: 101 });
        assert_eq!(crawl.pitch, 10);
        assert_eq!(l.widgets, Rect { x: 0, y: 360, w: 1920, h: 720 });
    }

    #[test]
    fn common_resolutions_partition_the_screen_and_keep_dots_visible() {
        for (w, h) in [(1920, 1080), (1366, 768), (1280, 720), (1024, 768), (1280, 1024), (800, 480)] {
            let l = default_layout(w, h);
            let crawl = l.crawl.unwrap();
            // Bands stack with no gaps or overlap and cover the screen.
            assert_eq!(l.ticker.band.y, 0);
            assert_eq!(crawl.band.y, l.ticker.band.bottom());
            assert_eq!(l.widgets.y, crawl.band.bottom());
            assert_eq!(l.widgets.bottom(), h);
            for g in [l.ticker, crawl] {
                // Grid fits inside its band and fills the width.
                assert!(g.origin.1 >= g.band.y && g.origin.1 + g.pitch * g.rows <= g.band.bottom(), "{w}x{h}");
                assert!(g.cols * g.pitch >= w);
                assert!(g.pitch >= 4, "{w}x{h}: pitch {} too small to draw a dot", g.pitch);
            }
        }
    }

    #[test]
    fn crawl_can_be_disabled() {
        let l = ScreenLayout::compute(1920, 1080, 0.25, 0.0, 19, 10);
        assert!(l.crawl.is_none());
        assert_eq!(l.ticker.band.h, 270);
        assert_eq!(l.widgets.y, 270);
    }

    #[test]
    fn fit_centered_keeps_square_cells() {
        let g = LedGrid::fit_centered(Rect { x: 100, y: 50, w: 1000, h: 300 }, 90, 20);
        assert_eq!(g.pitch, 11);
        assert_eq!(g.origin, (100 + (1000 - 990) / 2, 50 + (300 - 220) / 2));
    }
}
