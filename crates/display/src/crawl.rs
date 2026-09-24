//! The crawl under the ticker: flat text moving right to left behind a
//! fixed tag (TONIGHT, TODAY, UP NEXT), in the theme's style (ADR-0012).
//! See ADR-0007: LED is for the ticker only.
//!
//! The strip is drawn once per content change; the GPU scrolls it (the UI
//! shader's scroll mode), so a frame costs no CPU drawing or uploads.

use marqueet_core::Rgb;
use marqueet_core::theme::{Style, Theme};
use marqueet_core::ticker::{Part, Span, TickerSegment, Tint};

use crate::theme::Kit;
use crate::ui::{Canvas, Face, Fonts, TextStyle};

/// Labels the tag can show; its layer is sized for the widest.
const LABELS: [&str; 3] = ["TONIGHT", "TODAY", "UP NEXT"];

fn text_face(style: Style) -> (Face, f32) {
    match style {
        Style::Broadcast => (Face::Heavy, 0.46),
        Style::Ballpark => (Face::Stencil, 0.54),
        Style::Varsity => (Face::SemiBold, 0.46),
    }
}

fn tag_style(kit: &Kit, h: u32) -> TextStyle {
    let h = h as f32;
    match kit.style {
        Style::Broadcast => TextStyle::new(Face::Heavy, h * 0.46, kit.p.strip).tracking(h * 0.01),
        Style::Ballpark => TextStyle::new(Face::Stencil, h * 0.54, kit.p.accent).tracking(h * 0.02),
        Style::Varsity => TextStyle::new(Face::Collegiate, h * 0.38, kit.p.strip).tracking(h * 0.02),
    }
}

/// How far the broadcast tag's right edge leans.
fn slant(h: u32) -> f32 {
    h as f32 * 0.28
}

/// Width of the tag layer for a crawl `h` pixels tall.
pub fn tag_width(fonts: &Fonts, theme: &Theme, h: u32) -> u32 {
    let kit = Kit::new(theme);
    let style = tag_style(&kit, h);
    let widest = LABELS.iter().map(|l| fonts.measure(style.face, style.size, style.tracking, l)).fold(0.0, f32::max);
    let extra = if kit.style == Style::Broadcast { slant(h) } else { 0.0 };
    (widest + h as f32 * 0.9 + extra).ceil() as u32
}

/// Draws the tag filling `canvas`.
pub fn draw_tag(canvas: &mut Canvas, fonts: &mut Fonts, theme: &Theme, label: &str) {
    let kit = Kit::new(theme);
    canvas.clear();
    let (w, h) = (canvas.width, canvas.height);
    let (wf, hf) = (w as f32, h as f32);
    let face_w = match kit.style {
        Style::Broadcast => {
            // A dark block with a slanted right edge, like a TV lower third.
            let lean = slant(h);
            canvas.fill_polygon(&[(0.0, 0.0), (wf, 0.0), (wf - lean, hf), (0.0, hf)], kit.p.strip_text);
            wf - lean
        }
        Style::Ballpark => {
            canvas.fill_rect(0, 0, w as i32, h as i32, kit.p.strip);
            let edge = (hf * 0.06).max(1.0);
            canvas.fill_rect((wf - edge) as i32, 0, edge as i32 + 1, h as i32, kit.p.strip.scale(0.6));
            wf - edge
        }
        Style::Varsity => {
            canvas.fill_rect(0, 0, w as i32, h as i32, kit.p.strip_text);
            wf
        }
    };
    let style = tag_style(&kit, h);
    let text_w = fonts.measure(style.face, style.size, style.tracking, label);
    let baseline = hf / 2.0 + fonts.cap_height(style.face, style.size) / 2.0;
    canvas.text(fonts, (face_w - text_w) / 2.0, baseline, style, label);
}

/// One run of text in the strip.
struct Run {
    /// Space before it, px.
    gap: f32,
    text: String,
    style: TextStyle,
    /// Drawn as a small label in a box (league names).
    boxed: bool,
}

impl Run {
    fn width(&self, fonts: &Fonts) -> f32 {
        let w = fonts.measure(self.style.face, self.style.size, self.style.tracking, &self.text);
        self.gap + if self.boxed { w + self.style.size * 0.7 } else { w }
    }
}

fn runs(kit: &Kit, segments: &[TickerSegment], h: u32) -> Vec<Vec<Run>> {
    let (face, share) = text_face(kit.style);
    let size = h as f32 * share;
    let p = &kit.p;
    let caps = kit.style != Style::Varsity;
    let style = |s: &Span| match (s.tint, kit.style) {
        (Tint::Accent, Style::Ballpark) => (TextStyle::new(face, size, p.accent), false),
        (Tint::Accent, Style::Broadcast) => (TextStyle::new(Face::Heavy, size * 0.66, p.strip), true),
        (Tint::Accent, Style::Varsity) => (TextStyle::new(Face::Collegiate, size * 0.6, p.strip), true),
        (Tint::Dim, Style::Ballpark) => (TextStyle::new(face, size, p.strip_muted()), false),
        (Tint::Dim, _) => (TextStyle::new(Face::SemiBold, size, p.strip_muted()), false),
        _ => (TextStyle::new(face, size, p.strip_text), false),
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
                    let (style, boxed) = style(s);
                    let text = if caps { text.to_uppercase() } else { text.to_owned() };
                    (!text.is_empty()).then_some(Run { gap, text, style, boxed })
                })
                .collect()
        })
        .collect()
}

/// The mark between items: a rule, a bolt or a dot.
fn separator_width(kit: &Kit, h: u32) -> f32 {
    let h = h as f32;
    let mark = match kit.style {
        Style::Broadcast => (h * 0.03).max(2.0),
        Style::Ballpark => h * 0.16,
        Style::Varsity => h * 0.08,
    };
    h * 0.42 * 2.0 + mark
}

fn draw_separator(canvas: &mut Canvas, kit: &Kit, x: f32, h: u32) {
    let hf = h as f32;
    let gap = hf * 0.42;
    let p = &kit.p;
    match kit.style {
        Style::Broadcast => {
            let (w, bar) = ((hf * 0.03).max(2.0), hf * 0.44);
            canvas.fill_rect(
                (x + gap) as i32,
                ((hf - bar) / 2.0) as i32,
                w as i32,
                bar as i32,
                p.strip_text.mix(p.strip, 0.78),
            );
        }
        Style::Ballpark => {
            // A bolt head holding the painted panel on.
            let d = hf * 0.16;
            canvas.fill_round_rect(x + gap, (hf - d) / 2.0, d, d, d / 2.0, p.strip.scale(0.62));
            canvas.fill_round_rect(
                x + gap + d * 0.2,
                (hf - d) / 2.0 + d * 0.12,
                d * 0.4,
                d * 0.22,
                d * 0.1,
                p.strip.mix(Rgb::WHITE, 0.12),
            );
        }
        Style::Varsity => {
            let d = hf * 0.08;
            canvas.fill_round_rect(x + gap, (hf - d) / 2.0, d, d, d / 2.0, p.strip_muted());
        }
    }
}

/// Draws one period of the crawl: every segment followed by a separator, as
/// a strip `h` tall. Segments that would push it past `max_w` are dropped.
pub fn draw_strip(fonts: &mut Fonts, theme: &Theme, segments: &[TickerSegment], h: u32, max_w: u32) -> Canvas {
    let kit = Kit::new(theme);
    let items = runs(&kit, segments, h);
    let sep_w = separator_width(&kit, h);
    let mut widths = Vec::new();
    let mut total = 0.0;
    for item in &items {
        let w = item.iter().map(|r| r.width(fonts)).sum::<f32>() + sep_w;
        if total + w > max_w as f32 && !widths.is_empty() {
            break;
        }
        widths.push(w);
        total += w;
    }
    let width = (total.ceil() as u32).clamp(1, max_w.max(1));

    let mut canvas = Canvas::new(width, h);
    canvas.fill_rect(0, 0, width as i32, h as i32, kit.p.strip);
    if kit.style == Style::Ballpark {
        // The painted strip's shadowed bottom edge.
        let edge = (h / 16).max(1);
        canvas.fill_rect(0, (h - edge) as i32, width as i32, edge as i32, kit.p.strip.scale(0.66));
    }
    let hf = h as f32;
    let mut x = 0.0;
    for item in items.iter().take(widths.len()) {
        for run in item {
            x += run.gap;
            let style = run.style;
            let cap = fonts.cap_height(style.face, style.size);
            let baseline = hf / 2.0 + cap / 2.0;
            if run.boxed {
                let pad = style.size * 0.35;
                let text_w = fonts.measure(style.face, style.size, style.tracking, &run.text);
                let (bw, bh) = (text_w + pad * 2.0, cap + pad * 1.5);
                let r = if kit.style == Style::Varsity { bh * 0.2 } else { 0.0 };
                canvas.fill_round_rect(x, (hf - bh) / 2.0, bw, bh, r, kit.p.strip_text);
                canvas.text(fonts, x + pad, baseline, style, &run.text);
                x += bw;
            } else {
                x += canvas.text(fonts, x, baseline, style, &run.text);
            }
        }
        draw_separator(&mut canvas, &kit, x, h);
        x += sep_w;
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
    fn strip_has_league_team_and_time_colors_in_every_style() {
        let mut fonts = Fonts::new();
        let segs = [seg("a", "NFL", "NYS at BOS", "1:00 PM"), seg("b", "MLB", "SEA at HOU", "8:10 PM")];
        for style in Style::ALL {
            let theme = Theme::preset(style);
            let p = theme.palette;
            let c = draw_strip(&mut fonts, &theme, &segs, 70, 100_000);
            assert!(c.width > 400 && c.width < 1600, "{style:?} {}", c.width);
            assert!(has(&c, p.strip) && has(&c, p.strip_text) && has(&c, p.strip_muted()), "{style:?}");
            if style == Style::Ballpark {
                assert!(has(&c, p.accent), "painted league names");
            }
        }
    }

    #[test]
    fn strip_stops_at_whole_segments_when_too_long() {
        let mut fonts = Fonts::new();
        let theme = Theme::default();
        let one = draw_strip(&mut fonts, &theme, &[seg("a", "NFL", "NYS at BOS", "1:00 PM")], 70, 100_000).width;
        let many: Vec<_> = (0..50).map(|i| seg(&i.to_string(), "NFL", "NYS at BOS", "1:00 PM")).collect();
        let c = draw_strip(&mut fonts, &theme, &many, 70, one * 3 + one / 2);
        assert!(c.width <= one * 3 + 2 && c.width >= one * 3 - 2, "{} vs {}", c.width, one * 3);
    }

    #[test]
    fn tag_fits_every_label_in_every_style() {
        let mut fonts = Fonts::new();
        for style in Style::ALL {
            let theme = Theme::preset(style);
            let kit = Kit::new(&theme);
            let ink = tag_style(&kit, 70).paint.color;
            let w = tag_width(&fonts, &theme, 70);
            for label in LABELS {
                let mut c = Canvas::new(w, 70);
                draw_tag(&mut c, &mut fonts, &theme, label);
                let inked = |x: u32| (0..70).any(|y| c.pixel(x, y)[..3] == [ink.r, ink.g, ink.b]);
                assert!(!inked(0) && !inked(w - 1), "{style:?} {label}");
                assert!((0..w).any(inked), "{style:?} {label} drawn");
            }
        }
    }

    #[test]
    fn broadcast_tag_leans_right() {
        let mut fonts = Fonts::new();
        let theme = Theme::preset(Style::Broadcast);
        let w = tag_width(&fonts, &theme, 70);
        let mut c = Canvas::new(w, 70);
        draw_tag(&mut c, &mut fonts, &theme, "TONIGHT");
        assert_eq!(c.pixel(w - 2, 2)[3], 255, "top right filled");
        assert_eq!(c.pixel(w - 2, 67)[3], 0, "bottom right cut away");
    }
}
