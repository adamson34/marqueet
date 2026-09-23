//! Marqueet display: a native full-screen LED ticker.
//!
//! Phase 1 runs on built-in mock data. Examples:
//!
//! ```text
//! marqueet-display                          # 1920x1080 window
//! marqueet-display --size 1366x768 --led-color green
//! marqueet-display --fullscreen
//! marqueet-display --screenshot out.png --at 4.3 --scroll-to mock:nfl:1
//! ```

mod app;
mod band;
mod crawl;
mod feed;
mod gpu;
mod header;
mod mock;
mod render;
mod scene;
mod screenshot;
mod takeover;
mod ui;
mod widgets;

use std::path::PathBuf;

use clap::Parser;
use marqueet_core::Rgb;
use marqueet_core::config::{DisplayConfig, ScrollMode};
use marqueet_core::settings::{Settings, WidgetKind};
use marqueet_core::sports::HomeAway;
use scene::FeedSource;

#[derive(Debug, Parser)]
#[command(name = "marqueet-display", version, about = "Full-screen LED sports ticker")]
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

    /// LED color: amber, red, green, blue, white or #rrggbb.
    #[arg(long)]
    led_color: Option<Rgb>,

    /// Main ticker speed in LED columns per second.
    #[arg(long)]
    speed: Option<f32>,

    /// Crawl speed, in tenths of the crawl's height per second.
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

    /// Use built-in demo data instead of connecting to marqueet-server.
    #[arg(long)]
    mock: bool,

    /// marqueet-server feed URL.
    #[arg(long, value_name = "URL", default_value = feed::DEFAULT_URL, conflicts_with = "mock")]
    server: String,

    /// With --mock: seed for the demo data.
    #[arg(long, default_value_t = 7)]
    seed: u64,

    /// With --mock: the two widget slots, e.g. `game_of_the_day,standings`
    /// (game_of_the_day, scores, standings).
    #[arg(long, value_delimiter = ',', value_parser = parse_widget, requires = "mock")]
    widgets: Vec<WidgetKind>,

    /// Render one frame to this PNG file instead of opening a window.
    #[arg(long, value_name = "PNG", conflicts_with = "record")]
    screenshot: Option<PathBuf>,

    /// Render a sequence of PNG frames into this directory (for demo videos).
    #[arg(long, value_name = "DIR")]
    record: Option<PathBuf>,

    /// With --record: seconds to record.
    #[arg(long, default_value_t = 6.0)]
    duration: f64,

    /// With --record: frames per second.
    #[arg(long, default_value_t = 30)]
    fps: u32,

    /// Headless: simulated seconds before capturing (the first frame, when recording).
    #[arg(long, default_value_t = 6.0)]
    at: f64,

    /// Headless: when capture starts, jump the ticker so this segment id is at the left.
    #[arg(long, value_name = "SEGMENT_ID")]
    scroll_to: Option<String>,

    /// Headless: flash this ticker segment id (as if it just scored).
    #[arg(long, value_name = "SEGMENT_ID")]
    flash: Option<String>,

    /// With --flash: simulated second the flash starts (default: just before capture).
    #[arg(long)]
    flash_at: Option<f64>,

    /// Headless: score for a mock game, e.g. `mock:nfl:1:home:7`, as if a live
    /// scoring alert arrived (updates the score and flashes it).
    #[arg(long, value_name = "GAME_ID:home|away:POINTS", value_parser = parse_score)]
    score: Option<(String, HomeAway, u16)>,

    /// With --score: simulated second the score happens (default: just before capture).
    #[arg(long)]
    score_at: Option<f64>,
}

fn parse_score(s: &str) -> Result<(String, HomeAway, u16), String> {
    let mut parts = s.rsplitn(3, ':');
    let (Some(points), Some(side), Some(id)) = (parts.next(), parts.next(), parts.next()) else {
        return Err("expected GAME_ID:home|away:POINTS, e.g. mock:nfl:1:home:7".into());
    };
    let side = match side {
        "home" => HomeAway::Home,
        "away" => HomeAway::Away,
        _ => return Err("side must be home or away".into()),
    };
    let points = points.parse().map_err(|_| "points must be a whole number")?;
    Ok((id.to_owned(), side, points))
}

fn parse_widget(s: &str) -> Result<WidgetKind, String> {
    match s.trim() {
        "game_of_the_day" | "gotd" => Ok(WidgetKind::GameOfTheDay),
        "scores" => Ok(WidgetKind::Scores),
        "standings" => Ok(WidgetKind::Standings),
        other => Err(format!("unknown widget {other:?} (game_of_the_day, scores, standings)")),
    }
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
    fn source(&self) -> FeedSource {
        if self.mock {
            let widgets = if self.widgets.is_empty() { Settings::default().widgets } else { self.widgets.clone() };
            FeedSource::Mock { seed: self.seed, widgets }
        } else {
            FeedSource::Live { url: self.server.clone() }
        }
    }

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
    let output = match (&cli.screenshot, &cli.record) {
        (Some(png), _) => screenshot::Output::Frame(png.clone()),
        (None, Some(dir)) => screenshot::Output::Frames { dir: dir.clone(), duration: cli.duration, fps: cli.fps },
        (None, None) => return app::run(config, cli.size, cli.fullscreen, cli.source()),
    };
    screenshot::run(
        config,
        screenshot::Options {
            output,
            size: cli.size,
            at: cli.at,
            source: cli.source(),
            scroll_to: cli.scroll_to.clone(),
            flash: cli.flash.clone(),
            flash_at: cli.flash_at.unwrap_or(cli.at - 0.1),
            score: cli.score.clone(),
            score_at: cli.score_at.unwrap_or(cli.at - 0.1),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_score_specs() {
        assert_eq!(parse_score("mock:nfl:1:home:7"), Ok(("mock:nfl:1".into(), HomeAway::Home, 7)));
        assert_eq!(parse_score("g:away:3"), Ok(("g".into(), HomeAway::Away, 3)));
        assert!(parse_score("mock:nfl:1:left:7").is_err());
        assert!(parse_score("7").is_err());
    }

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
