//! User-facing display settings. Persisted by the server (Phase 4); for now
//! the display reads them from command-line flags.

use serde::{Deserialize, Serialize};

use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollMode {
    /// Content jumps one LED column at a time, like a real sign.
    #[default]
    Stepped,
    /// LEDs cross-fade between columns for smoother motion.
    Smooth,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Fraction of screen height used by ticker + crawl.
    pub ticker_ratio: f32,
    /// Fraction of the ticker area given to the crawl.
    pub crawl_share: f32,
    /// LED rows in the main ticker. 17+ allows stacked two-line games.
    pub ticker_rows: u32,
    /// LED rows in the crawl.
    pub crawl_rows: u32,
    pub led_color: Rgb,
    /// Main ticker speed in LED columns per second.
    pub ticker_speed: f32,
    /// Crawl speed in LED columns per second.
    pub crawl_speed: f32,
    pub scroll_mode: ScrollMode,
    /// Glow (bloom) strength, 0 = off.
    pub glow: f32,
    /// Flicker amount, 0 = off.
    pub flicker: f32,
    /// Lit dot diameter as a fraction of the LED pitch.
    pub dot_size: f32,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            ticker_ratio: 1.0 / 3.0,
            crawl_share: 0.28,
            ticker_rows: 19,
            crawl_rows: 10,
            led_color: Rgb::AMBER,
            ticker_speed: 22.0,
            crawl_speed: 30.0,
            scroll_mode: ScrollMode::Stepped,
            glow: 0.55,
            flicker: 0.25,
            dot_size: 0.78,
        }
    }
}

impl DisplayConfig {
    /// Clamps every field into its supported range.
    pub fn sanitized(mut self) -> Self {
        let d = Self::default();
        let clamp = |v: f32, lo: f32, hi: f32, default: f32| if v.is_finite() { v.clamp(lo, hi) } else { default };
        self.ticker_ratio = clamp(self.ticker_ratio, 0.15, 0.6, d.ticker_ratio);
        self.crawl_share = clamp(self.crawl_share, 0.0, 0.5, d.crawl_share);
        self.ticker_rows = self.ticker_rows.clamp(9, 48);
        self.crawl_rows = self.crawl_rows.clamp(9, 24);
        self.ticker_speed = clamp(self.ticker_speed, 1.0, 200.0, d.ticker_speed);
        self.crawl_speed = clamp(self.crawl_speed, 1.0, 200.0, d.crawl_speed);
        self.glow = clamp(self.glow, 0.0, 2.0, d.glow);
        self.flicker = clamp(self.flicker, 0.0, 1.0, d.flicker);
        self.dot_size = clamp(self.dot_size, 0.3, 1.0, d.dot_size);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_already_sane() {
        assert_eq!(DisplayConfig::default().sanitized(), DisplayConfig::default());
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let c =
            DisplayConfig { ticker_ratio: 5.0, ticker_rows: 2, glow: f32::NAN, flicker: -1.0, ..Default::default() }
                .sanitized();
        assert_eq!(c.ticker_ratio, 0.6);
        assert_eq!(c.ticker_rows, 9);
        assert_eq!(c.glow, DisplayConfig::default().glow);
        assert_eq!(c.flicker, 0.0);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let c: DisplayConfig = serde_json::from_str(r##"{"led_color":"#00ff00"}"##).unwrap();
        assert_eq!(c.led_color, Rgb::new(0, 255, 0));
        assert_eq!(c.ticker_rows, DisplayConfig::default().ticker_rows);
    }
}
