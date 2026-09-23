//! The crawl under the ticker: flat condensed text moving right to left
//! behind a fixed tag (TONIGHT, TODAY, UP NEXT). See ADR-0007: LED is for the
//! ticker only.
//!
//! The strip is drawn once per content change; the GPU scrolls it (the UI
//! shader's scroll mode), so a frame costs no CPU drawing or uploads.

use marqueet_core::Rgb;
use marqueet_core::ticker::{Part, Span, TickerSegment, Tint};

use crate::ui::{Canvas, Fonts, TextStyle, Weight};

pub const BG: Rgb = Rgb::new(0x17, 0x18, 0x1c);
const LINE: Rgb = Rgb::new(0x2a, 0x2b, 0x31);
pub const TAG: Rgb = Rgb::new(0xf2, 0xa9, 0x3b);
const TAG_TEXT: Rgb = Rgb::new(0x11, 0x11, 0x11);
const TEXT: Rgb = Rgb::new(0xe8, 0xe9, 0xec);
const MUTED: Rgb = Rgb::new(0x8b, 0x8d, 0x96);
const SEPARATOR: Rgb = Rgb::new(0x44, 0x45, 0x4c);

/// Labels the tag can show; its layer is sized for the widest.
const LABELS: [&str; 3] = ["TONIGHT", "TODAY", "UP NEXT"];

fn text_size(h: u32) -> f32 {
    h as f32 * 0.46
}

fn tag_style(h: u32) -> TextStyle {
    TextStyle::new(Weight::SemiBold, text_size(h), TAG_TEXT).tracking(h as f32 * 0.03)
}

/// Width of the tag layer for a crawl `h` pixels tall.
pub fn tag_width(fonts: &Fonts, h: u32) -> u32 {
    let style = tag_style(h);
    let widest = LABELS.iter().map(|l| fonts.measure(style.weight, style.size, style.tracking, l)).fold(0.0, f32::max);
    (widest + h as f32 * 0.9).ceil() as u32
}

/// Draws the tag (amber block, dark centered label) filling `canvas`.
pub fn draw_tag(canvas: &mut Canvas, fonts: &mut Fonts, label: &str) {
    canvas.clear();
    let (w, h) = (canvas.width, canvas.height);
    canvas.fill_rect(0, 0, w as i32, h as i32, TAG);
    let style = tag_style(h);
    let text_w = fonts.measure(style.weight, style.size, style.tracking, label);
    let baseline = h as f32 / 2.0 + fonts.cap_height(style.weight, style.size) / 2.0;
    canvas.text(fonts, (w as f32 - text_w) / 2.0, baseline, style, label);
}

/// One run of text in the strip.
struct Run<'a> {
    /// Space before it, px.
    gap: f32,
    text: &'a str,
    style: TextStyle,
}

fn runs<'a>(segments: &'a [TickerSegment], h: u32) -> Vec<Vec<Run<'a>>> {
    let size = text_size(h);
    let style = |s: &Span| match s.tint {
        Tint::Accent => TextStyle::new(Weight::SemiBold, size, TAG).tracking(h as f32 * 0.02),
        Tint::Dim => TextStyle::new(Weight::Medium, size, MUTED),
        _ => TextStyle::new(Weight::Medium, size, TEXT),
    };
    segments
        .iter()
        .map(|seg| {
            seg.parts
                .iter()
                .flat_map(|p| match p {
                    Part::Text { spans } => spans.iter().collect::<Vec<_>>(),
                    Part::Stack { top, bottom, .. } => top.iter().chain(bottom).collect(),
                    Part::Gap { .. } | Part::Icon { .. } => Vec::new(),
                })
                .filter_map(|s| {
                    let text = s.text.trim();
                    let lead = s.text.len() - s.text.trim_start().len();
                    let gap = match lead {
                        0 => 0.0,
                        1 => size * 0.28,
                        _ => size * 0.75,
                    };
                    (!text.is_empty()).then(|| Run { gap, text, style: style(s) })
                })
                .collect()
        })
        .collect()
}

/// Draws one period of the crawl: every segment followed by a separator, as
/// a strip `h` tall. Segments that would push it past `max_w` are dropped.
pub fn draw_strip(fonts: &mut Fonts, segments: &[TickerSegment], h: u32, max_w: u32) -> Canvas {
    let size = text_size(h);
    let sep_gap = size * 0.9;
    let sep = TextStyle::new(Weight::Medium, size, SEPARATOR);
    let items = runs(segments, h);

    let measure = |fonts: &Fonts, run: &Run<'_>| {
        run.gap + fonts.measure(run.style.weight, run.style.size, run.style.tracking, run.text)
    };
    let sep_w = sep_gap * 2.0 + fonts.measure(sep.weight, sep.size, sep.tracking, "|");
    let mut widths = Vec::new();
    let mut total = 0.0;
    for item in &items {
        let w = item.iter().map(|r| measure(fonts, r)).sum::<f32>() + sep_w;
        if total + w > max_w as f32 && !widths.is_empty() {
            break;
        }
        widths.push(w);
        total += w;
    }
    let width = (total.ceil() as u32).clamp(1, max_w.max(1));

    let mut canvas = Canvas::new(width, h);
    canvas.fill_rect(0, 0, width as i32, h as i32, BG);
    let line = (h / 70).max(1);
    canvas.fill_rect(0, (h - line) as i32, width as i32, line as i32, LINE);
    let baseline = h as f32 / 2.0 + fonts.cap_height(Weight::Medium, size) / 2.0;
    let mut x = 0.0;
    for item in items.iter().take(widths.len()) {
        for run in item {
            x += run.gap;
            x += canvas.text(fonts, x, baseline, run.style, run.text);
        }
        x += sep_gap;
        x += canvas.text(fonts, x, baseline, sep, "|");
        x += sep_gap;
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: &str, league: &str, teams: &str, time: &str) -> TickerSegment {
        TickerSegment {
            id: id.into(),
            parts: vec![Part::text(vec![
                Span::new(league, Tint::Accent),
                Span::primary(format!(" {teams}")),
                Span::dim(format!("  {time}")),
            ])],
        }
    }

    fn has(c: &Canvas, color: Rgb) -> bool {
        (0..c.width).any(|x| (0..c.height).any(|y| c.pixel(x, y) == [color.r, color.g, color.b, 255]))
    }

    #[test]
    fn strip_has_league_team_and_time_colors() {
        let mut fonts = Fonts::new();
        let segs = [seg("a", "NFL", "NYJ at NE", "1:00 PM"), seg("b", "MLB", "SEA at HOU", "8:10 PM")];
        let c = draw_strip(&mut fonts, &segs, 70, 100_000);
        assert!(c.width > 400 && c.width < 1400, "{}", c.width);
        assert!(has(&c, TAG) && has(&c, TEXT) && has(&c, MUTED) && has(&c, BG));
    }

    #[test]
    fn strip_stops_at_whole_segments_when_too_long() {
        let mut fonts = Fonts::new();
        let one = draw_strip(&mut fonts, &[seg("a", "NFL", "NYJ at NE", "1:00 PM")], 70, 100_000).width;
        let many: Vec<_> = (0..50).map(|i| seg(&i.to_string(), "NFL", "NYJ at NE", "1:00 PM")).collect();
        let c = draw_strip(&mut fonts, &many, 70, one * 3 + one / 2);
        assert!(c.width <= one * 3 + 2 && c.width >= one * 3 - 2, "{} vs {}", c.width, one * 3);
    }

    #[test]
    fn tag_fits_every_label() {
        let mut fonts = Fonts::new();
        let w = tag_width(&fonts, 70);
        for label in LABELS {
            let mut c = Canvas::new(w, 70);
            draw_tag(&mut c, &mut fonts, label);
            // Dark text never touches the left or right edge.
            let dark = |x: u32| (0..70).any(|y| c.pixel(x, y)[0] < 0x80);
            assert!(!dark(0) && !dark(w - 1), "{label}");
            assert!((0..w).any(dark), "{label} drawn");
        }
    }
}
