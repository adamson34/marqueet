//! Tickadee display: a native full-screen LED ticker.
//!
//! Phase 1 runs on built-in mock data. Examples:
//!
//! ```text
//! tickadee-display                          # 1920x1080 window
//! tickadee-display --size 1366x768 --led-color green
//! tickadee-display --fullscreen
//! tickadee-display --screenshot out.png --at 4.3 --scroll-to mock:nfl:1
//! ```

mod app;
mod band;
mod gpu;
mod mock;
mod render;
mod scene;
mod screenshot;

use std::path::PathBuf;

use clap::Parser;
use tickadee_core::Rgb;
use tickadee_core::config::{DisplayConfig, ScrollMode};

#[derive(Debug, Parser)]
#[command(name = "tickadee-display", version, about = "Full-screen LED sports ticker")]
struct Cli {
    /// Window size in physical pixels, e.g. 1366x768 (ignored with --fullscreen).
    #[arg(long, default_value = "1920x1080", value_parser = parse_size)]
    size: (u32, u32),

    /// Borderless full screen on the primary monitor.
    #[arg(long)]
    fullscreen: bool,

    /// Fraction of the screen height used by the ticker and crawl.
    #[arg(long)]
    ticker_ratio: Option<f32>,

    /// Fraction of the ticker area used by the crawl (0 hides it).
    #[arg(long)]
    crawl_share: Option<f32>,

    /// LED rows in the main ticker (17+ shows two-line game blocks).
    #[arg(long)]
    ticker_rows: Option<u32>,

    /// LED rows in the crawl.
    #[arg(long)]
    crawl_rows: Option<u32>,

    /// LED color: amber, red, green, blue, white or #rrggbb.
    #[arg(long)]
    led_color: Option<Rgb>,

    /// Main ticker speed in LED columns per second.
    #[arg(long)]
    speed: Option<f32>,

    /// Crawl speed in LED columns per second.
    #[arg(long)]
    crawl_speed: Option<f32>,

    /// Cross-fade between LED columns instead of stepping.
    #[arg(long)]
    smooth: bool,

    /// Glow strength (0 = off).
    #[arg(long)]
    glow: Option<f32>,

    /// Flicker amount (0 = off).
    #[arg(long)]
    flicker: Option<f32>,

    /// Lit dot diameter as a fraction of the LED pitch.
    #[arg(long)]
    dot_size: Option<f32>,

    /// Seed for the mock feed.
    #[arg(long, default_value_t = 7)]
    seed: u64,

    /// Render one frame to this PNG file instead of opening a window.
    #[arg(long, value_name = "PNG")]
    screenshot: Option<PathBuf>,

    /// With --screenshot: seconds of simulated time before capturing.
    #[arg(long, default_value_t = 2.0)]
    at: f64,

    /// With --screenshot: scroll the ticker so this segment id is visible.
    #[arg(long, value_name = "SEGMENT_ID")]
    scroll_to: Option<String>,

    /// With --screenshot: flash this segment id, `--flash-age` seconds before capture.
    #[arg(long, value_name = "SEGMENT_ID")]
    flash: Option<String>,

    /// With --flash: how far into the flash animation to capture.
    #[arg(long, default_value_t = 0.1)]
    flash_age: f64,
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once(['x', 'X']).ok_or("expected WIDTHxHEIGHT, e.g. 1366x768")?;
    let w: u32 = w.trim().parse().map_err(|_| "bad width")?;
    let h: u32 = h.trim().parse().map_err(|_| "bad height")?;
    if !(320..=8192).contains(&w) || !(240..=8192).contains(&h) {
        return Err("size out of range".into());
    }
    Ok((w, h))
}

impl Cli {
    fn config(&self) -> DisplayConfig {
        let mut c = DisplayConfig::default();
        macro_rules! set {
            ($($field:ident <- $opt:ident),* $(,)?) => {
                $(if let Some(v) = self.$opt { c.$field = v; })*
            };
        }
        set!(
            ticker_ratio <- ticker_ratio,
            crawl_share <- crawl_share,
            ticker_rows <- ticker_rows,
            crawl_rows <- crawl_rows,
            led_color <- led_color,
            ticker_speed <- speed,
            crawl_speed <- crawl_speed,
            glow <- glow,
            flicker <- flicker,
            dot_size <- dot_size,
        );
        if self.smooth {
            c.scroll_mode = ScrollMode::Smooth;
        }
        c.sanitized()
    }
}

fn main() -> render::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
    )
    .init();
    let cli = Cli::parse();
    let config = cli.config();
    match &cli.screenshot {
        Some(path) => screenshot::run(
            config,
            screenshot::Options {
                path: path.clone(),
                size: cli.size,
                at: cli.at,
                seed: cli.seed,
                scroll_to: cli.scroll_to.clone(),
                flash: cli.flash.clone(),
                flash_age: cli.flash_age,
            },
        ),
        None => app::run(config, cli.size, cli.fullscreen, cli.seed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("1366x768"), Ok((1366, 768)));
        assert_eq!(parse_size("1024X768"), Ok((1024, 768)));
        assert!(parse_size("1366").is_err());
        assert!(parse_size("10x10").is_err());
    }

    #[test]
    fn flags_override_defaults_and_are_sanitized() {
        let cli = Cli::parse_from(["x", "--led-color", "green", "--ticker-rows", "500", "--smooth"]);
        let c = cli.config();
        assert_eq!(c.led_color, Rgb::GREEN);
        assert_eq!(c.ticker_rows, 48);
        assert_eq!(c.scroll_mode, ScrollMode::Smooth);
    }
}
