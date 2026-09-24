//! The weather card and its icons, drawn from circles and rounded rects on
//! the UI canvas (no image assets).

use marqueet_core::Rgb;
use marqueet_core::weather::Condition;
use marqueet_core::widgets::WeatherView;

use crate::theme::Kit;
use crate::ui::{Align, Canvas, Face, Fonts, Paint, TextStyle};
use crate::widgets::Card;

pub const SUN: Rgb = Rgb::new(0xf7, 0xb7, 0x3b);
const MOON: Rgb = Rgb::new(0xe9, 0xe4, 0xd4);
const CLOUD: Rgb = Rgb::new(0xc9, 0xcb, 0xd2);
const STORM_CLOUD: Rgb = Rgb::new(0x8e, 0x91, 0x9b);
pub const RAIN: Rgb = Rgb::new(0x5a, 0xa0, 0xff);
const SNOW: Rgb = Rgb::new(0xf4, 0xf6, 0xfa);

fn circle(c: &mut Canvas, cx: f32, cy: f32, r: f32, paint: impl Into<Paint>) {
    c.fill_round_rect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, paint);
}

fn sun(c: &mut Canvas, x: f32, y: f32, size: f32) {
    let (cx, cy) = (x + size / 2.0, y + size / 2.0);
    circle(c, cx, cy, size * 0.24, SUN);
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        circle(c, cx + a.cos() * size * 0.39, cy + a.sin() * size * 0.39, size * 0.05, SUN);
    }
}

/// A crescent: a disc with a card-colored disc over one side.
fn moon(c: &mut Canvas, x: f32, y: f32, size: f32, background: Rgb) {
    let (cx, cy, r) = (x + size / 2.0, y + size / 2.0, size * 0.3);
    circle(c, cx, cy, r, MOON);
    circle(c, cx + r * 0.55, cy - r * 0.35, r * 0.85, background);
}

fn cloud(c: &mut Canvas, x: f32, y: f32, size: f32, color: Rgb) {
    let s = size;
    c.fill_round_rect(x + s * 0.12, y + s * 0.5, s * 0.76, s * 0.26, s * 0.13, color);
    circle(c, x + s * 0.34, y + s * 0.53, s * 0.17, color);
    circle(c, x + s * 0.55, y + s * 0.43, s * 0.23, color);
    circle(c, x + s * 0.74, y + s * 0.58, s * 0.14, color);
}

/// Draws the icon for `condition` in a `size` x `size` box at (x, y).
pub fn icon(c: &mut Canvas, x: f32, y: f32, size: f32, condition: Condition, day: bool, background: Rgb) {
    let s = size;
    match condition {
        Condition::Clear if day => sun(c, x, y, s),
        Condition::Clear => moon(c, x, y, s, background),
        Condition::PartlyCloudy => {
            if day {
                sun(c, x - s * 0.14, y - s * 0.16, s * 0.8);
            } else {
                moon(c, x - s * 0.12, y - s * 0.14, s * 0.8, background);
            }
            cloud(c, x + s * 0.06, y + s * 0.08, s * 0.94, CLOUD);
        }
        Condition::Cloudy => cloud(c, x, y, s, CLOUD),
        Condition::Fog => {
            for (i, w) in [0.7, 0.8, 0.6].into_iter().enumerate() {
                let bar_y = y + s * (0.3 + i as f32 * 0.17);
                c.fill_round_rect(x + s * (1.0 - w) / 2.0, bar_y, s * w, s * 0.09, s * 0.045, CLOUD);
            }
        }
        Condition::Drizzle | Condition::Rain => {
            cloud(c, x, y - s * 0.12, s, CLOUD);
            let len = if condition == Condition::Rain { 0.2 } else { 0.1 };
            for i in 0..3 {
                let dx = x + s * (0.3 + i as f32 * 0.2);
                c.fill_round_rect(dx, y + s * 0.72, s * 0.06, s * len, s * 0.03, RAIN);
            }
        }
        Condition::Snow => {
            cloud(c, x, y - s * 0.12, s, CLOUD);
            for (i, (fx, fy)) in
                [(0.3, 0.76), (0.5, 0.86), (0.7, 0.76), (0.4, 0.95), (0.6, 0.95)].into_iter().enumerate()
            {
                let r = if i < 3 { 0.05 } else { 0.04 };
                circle(c, x + s * fx, y + s * fy, s * r, SNOW);
            }
        }
        Condition::Thunder => {
            cloud(c, x, y - s * 0.12, s, STORM_CLOUD);
            // A bolt as a staircase of blocks, like the LED digits.
            let b = s * 0.075;
            for (bx, by) in [(0.52, 0.62), (0.46, 0.7), (0.4, 0.78), (0.52, 0.78), (0.46, 0.86), (0.4, 0.94)] {
                c.fill_rect((x + s * bx) as i32, (y + s * by) as i32, b.ceil() as i32, b.ceil() as i32, SUN);
            }
        }
    }
}

pub fn draw(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, card: Card, s: f32, v: &WeatherView) {
    let (x, y, w, h) = (card.x, card.y, card.w, card.h);
    let p = &kit.p;
    let background = p.panel;
    kit.card(canvas, fonts, card, s, &v.title);
    let place = TextStyle::new(Face::SemiBold, 28.0 * s, p.soft()).align(Align::Right);
    canvas.text(fonts, x + w - 40.0 * s, y + 62.0 * s, place, &v.place);
    // Open-Meteo's license (CC BY 4.0) asks for credit.
    let credit = TextStyle::new(Face::Medium, 18.0 * s, p.muted).align(Align::Right);
    canvas.text(fonts, x + w - 40.0 * s, y + 88.0 * s, credit, "via Open-Meteo");

    // Now: icon, big temperature, summary; details on the right when wide.
    let icon_size = (170.0 * s).min(h * 0.3);
    let top = y + 96.0 * s;
    icon(canvas, x + 40.0 * s, top, icon_size, v.condition, v.day, background);
    let temp_x = x + 40.0 * s + icon_size + 28.0 * s;
    let temp = TextStyle::new(kit.number_face(), 120.0 * s, p.text);
    canvas.text(fonts, temp_x, top + icon_size * 0.66, temp, &v.temperature);
    let summary = TextStyle::new(Face::Medium, 34.0 * s, p.soft());
    canvas.text(fonts, temp_x, top + icon_size * 0.66 + 50.0 * s, summary, &v.summary);
    let detail = TextStyle::new(Face::Medium, 28.0 * s, p.muted);
    if w > 800.0 * s {
        for (i, d) in v.details.iter().enumerate() {
            let dy = top + 52.0 * s + i as f32 * 44.0 * s;
            canvas.text(fonts, x + w - 40.0 * s, dy, detail.align(Align::Right), d);
        }
    } else {
        let line = v.details.join("  ·  ");
        canvas.text(fonts, x + 40.0 * s, top + icon_size + 40.0 * s, detail, &line);
    }

    // Forecast row along the bottom, if there's room.
    let row_top = y + h - 200.0 * s;
    if row_top < top + icon_size + 60.0 * s || v.days.is_empty() {
        return;
    }
    canvas.fill_rect((x + 40.0 * s) as i32, row_top as i32, (w - 80.0 * s) as i32, s.max(1.0) as i32, p.rule());
    let n = v.days.len() as f32;
    let span = w - 80.0 * s;
    for (i, d) in v.days.iter().enumerate() {
        let cx = x + 40.0 * s + span * (i as f32 + 0.5) / n;
        let name = TextStyle::new(Face::Medium, 24.0 * s, p.muted).tracking(1.5 * s).align(Align::Center);
        canvas.text(fonts, cx, row_top + 42.0 * s, name, &d.name);
        let small = 58.0 * s;
        icon(canvas, cx - small / 2.0, row_top + 56.0 * s, small, d.condition, true, background);
        let hi = TextStyle::new(Face::SemiBold, 30.0 * s, p.text).align(Align::Right);
        let lo = TextStyle::new(Face::Medium, 30.0 * s, p.muted);
        canvas.text(fonts, cx - 4.0 * s, row_top + 150.0 * s, hi, &d.high);
        canvas.text(fonts, cx + 4.0 * s, row_top + 150.0 * s, lo, &d.low);
        if let Some(p) = &d.precipitation {
            let style = TextStyle::new(Face::SemiBold, 22.0 * s, RAIN).align(Align::Center);
            canvas.text(fonts, cx, row_top + 180.0 * s, style, p);
        }
    }
}
