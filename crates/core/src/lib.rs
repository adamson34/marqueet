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
pub mod sports;
pub mod ticker;

pub use color::Rgb;
