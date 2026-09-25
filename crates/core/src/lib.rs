//! Shared, I/O-free building blocks for Marqueet.
//!
//! - [`sports`]: the normalized game schema every sports provider maps into,
//!   plus formatting of games into ticker segments.
//! - [`ticker`]: generic ticker content (segments, spans, tints) and the LED
//!   strip rasterizer. Any source (sports, weather, stocks) produces segments.
//! - [`protocol`]: server → display WebSocket messages.
//! - [`provider`]: the data-provider plugin interface (fetch + normalize).
//! - [`events`]: the event engine (touchdowns, home runs, goals…) from
//!   consecutive snapshots.
//! - [`alert`]: flashes and takeovers raised by sources.
//! - [`font`]: the hand-drawn LED bitmap font and its Scale2x large variant.
//! - [`logo`]: the parakeet mark as a dot grid, rendered to SVG and LEDs.
//! - [`layout`]: screen partitioning into ticker, crawl and widget area.
//! - [`widgets`]: ready-to-draw widget view models (game of the day, scores).
//! - [`settings`]: user settings (leagues, favorites, takeovers, widgets, quiet hours).
//! - [`config`]: user-facing display settings.
//! - [`team_art`]: people's own team colors and logos (none ship).
//! - [`theme`]: display themes (style + palette) and team-color math.

pub mod alert;
pub mod color;
pub mod config;
pub mod events;
pub mod fantasy;
pub mod feeds;
pub mod font;
pub mod icons;
pub mod layout;
pub mod logo;
pub mod protocol;
pub mod provider;
pub mod settings;
pub mod sports;
pub mod team_art;
pub mod test_alerts;
pub mod theme;
pub mod ticker;
pub mod weather;
pub mod widgets;

pub use color::Rgb;
