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

pub mod alert;
pub mod color;
pub mod config;
pub mod events;
pub mod font;
pub mod layout;
pub mod logo;
pub mod protocol;
pub mod provider;
pub mod settings;
pub mod sports;
pub mod ticker;
pub mod weather;
pub mod widgets;

pub use color::Rgb;
