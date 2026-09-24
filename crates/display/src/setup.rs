//! The first-boot screen: while the device has no admin password, the widget
//! area shows how to set it up: a QR code for the setup page, its address,
//! and the one-time code in LED digits.

use marqueet_core::Rgb;
use marqueet_core::protocol::SetupInfo;
use marqueet_core::theme::Theme;
use qrcodegen::{QrCode, QrCodeEcc};

use crate::theme::Kit;
use crate::ui::{Align, Canvas, Face, Fonts, TextStyle};
use crate::widgets::Card;

/// Draws the QR code for `text` as a white square of `size` px (with a
/// quiet zone) at (x, y). Returns false if the text can't be encoded.
pub fn qr(canvas: &mut Canvas, x: f32, y: f32, size: f32, text: &str) -> bool {
    let Ok(code) = QrCode::encode_text(text, QrCodeEcc::Medium) else { return false };
    let n = code.size();
    // Four modules of white around the code, as scanners expect.
    let module = (size / (n + 8) as f32).floor().max(1.0);
    let side = module * (n + 8) as f32;
    canvas.fill_round_rect(x, y, side, side, module * 2.0, Rgb::WHITE);
    for row in 0..n {
        for col in 0..n {
            if code.get_module(col, row) {
                let (mx, my) = (x + module * (col + 4) as f32, y + module * (row + 4) as f32);
                canvas.fill_rect(mx as i32, my as i32, module as i32, module as i32, Rgb::BLACK);
            }
        }
    }
    true
}

/// The setup card across the whole widget area.
pub fn draw(canvas: &mut Canvas, fonts: &mut Fonts, info: &SetupInfo, theme: &Theme) {
    let kit = Kit::new(theme);
    let p = &kit.p;
    canvas.clear();
    canvas.fill_rect(0, 0, canvas.width as i32, canvas.height as i32, p.ground);
    let (w, h) = (canvas.width as f32, canvas.height as f32);
    let s = (w / 1920.0).min(h / 648.0);
    let (m, top) = (40.0 * s, 26.0 * s);
    let (cw, ch) = (w - 2.0 * m, h - 52.0 * s);
    kit.panel(canvas, Card { x: m, y: top, w: cw, h: ch }, s);

    // QR code on the left, as tall as the card allows.
    let qr_size = (ch - 80.0 * s).min(cw * 0.36);
    let qx = m + 48.0 * s;
    let url = info.urls.first().map_or("", String::as_str);
    let has_qr = !url.is_empty() && qr(canvas, qx, top + (ch - qr_size) / 2.0, qr_size, url);
    let tx = if has_qr { qx + qr_size + 64.0 * s } else { m + 64.0 * s };

    let title = TextStyle::new(kit.title_face(), 44.0 * s, p.accent).tracking(1.0 * s);
    canvas.text(fonts, tx, top + 86.0 * s, title, "SET UP MARQUEET");
    let step = TextStyle::new(Face::Medium, 30.0 * s, p.soft());
    let big = TextStyle::new(Face::SemiBold, 44.0 * s, p.text);
    let quiet = TextStyle::new(Face::Medium, 26.0 * s, p.muted);

    let mut y = top + 150.0 * s;
    canvas.text(
        fonts,
        tx,
        y,
        step,
        if has_qr { "1  Scan, or open on your phone or laptop:" } else { "1  Open on your phone or laptop:" },
    );
    y += 56.0 * s;
    canvas.text(fonts, tx, y, big, url.strip_prefix("http://").unwrap_or(url));
    for other in info.urls.iter().skip(1).take(1) {
        y += 40.0 * s;
        canvas.text(fonts, tx, y, quiet, &format!("or {}", other.strip_prefix("http://").unwrap_or(other)));
    }
    y += 76.0 * s;
    canvas.text(fonts, tx, y, step, "2  Enter this code, then choose a password:");
    let px = 13.0 * s;
    canvas.led_text(tx, y + 26.0 * s, px, p.accent, true, &info.code);
    let note = TextStyle::new(Face::Medium, 22.0 * s, p.muted).align(Align::Right);
    canvas.text(fonts, m + cw - 32.0 * s, top + ch - 24.0 * s, note, "This screen goes away once a password is set.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> SetupInfo {
        SetupInfo {
            code: "482193".into(),
            urls: vec!["http://marqueet.local:7878/setup".into(), "http://192.168.1.20:7878/setup".into()],
        }
    }

    #[test]
    fn qr_codes_scan_as_black_on_white() {
        let mut c = Canvas::new(300, 300);
        assert!(qr(&mut c, 0.0, 0.0, 300.0, "http://marqueet.local:7878/setup"));
        assert_eq!(c.pixel(150, 2)[..3], [255, 255, 255], "quiet zone");
        let dark = (0..300)
            .flat_map(|x| (0..300).map(move |y| (x, y)))
            .filter(|&(x, y)| c.pixel(x, y)[..3] == [0, 0, 0])
            .count();
        assert!(dark > 5000, "modules drawn: {dark}");
        // Finder pattern: the top-left corner inside the quiet zone is dark.
        let module = (300.0 / (25.0 + 8.0f32)).floor() as u32; // version 2 = 25 modules
        assert_eq!(c.pixel(module * 4 + 1, module * 4 + 1)[..3], [0, 0, 0]);
    }

    #[test]
    fn draws_the_code_in_led_digits() {
        let mut fonts = Fonts::new();
        let mut c = Canvas::new(1920, 648);
        let theme = Theme::default();
        draw(&mut c, &mut fonts, &info(), &theme);
        let a = theme.palette.accent;
        let amber = (900..1900).any(|x| (380..560).any(|y| c.pixel(x, y)[..3] == [a.r, a.g, a.b]));
        assert!(amber, "LED code on the right");
        assert_eq!(c.pixel(100, 324)[..3], [255, 255, 255], "QR code on the left");
    }
}
