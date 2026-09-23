//! Flat UI (header bar, widget cards, takeover text) drawn on a CPU canvas
//! with vector text, then uploaded to the GPU when it changes (ADR-0007).

pub mod canvas;
pub mod text;

pub use canvas::{Align, Canvas, Paint, TextStyle};
pub use text::{Fonts, Weight};
