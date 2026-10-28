//! The camera card (ADR-0016): a camera's latest picture under its name,
//! several cameras taking turns. Pictures arrive as snapshots by key; the
//! display never learns where they came from (ADR-0003).

use marqueet_core::camera::Snapshots;
use marqueet_core::team_art::Image;
use marqueet_core::widgets::{CameraFeed, CameraView};

use crate::theme::Kit;
use crate::ui::{Align, Canvas, Face, Fonts, TextStyle};
use crate::widgets::Card;

/// Which camera is up at `secs` on the wall clock.
pub fn turn(view: &CameraView, secs: u64) -> usize {
    view.up(secs)
}

/// Draws the camera that's up at `secs`, letterboxed to keep its shape, or
/// a note while its first picture is on the way.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    kit: &Kit,
    card: Card,
    s: f32,
    view: &CameraView,
    snapshots: &Snapshots,
    secs: u64,
) {
    let Some(feed) = view.feeds.get(turn(view, secs)) else { return };
    kit.card(canvas, fonts, card, s, &feed.title);
    if view.feeds.len() > 1 {
        dots(canvas, kit, card, s, view.feeds.len(), turn(view, secs));
    }
    let (inset, top) = (36.0 * s, 92.0 * s);
    let area = Card { x: card.x + inset, y: card.y + top, w: card.w - 2.0 * inset, h: card.h - top - inset };
    canvas.fill_round_rect(area.x, area.y, area.w, area.h, kit.radius(s), kit.p.plate);
    picture(canvas, fonts, kit, area, s, feed, snapshots);
}

fn picture(
    canvas: &mut Canvas,
    fonts: &mut Fonts,
    kit: &Kit,
    area: Card,
    s: f32,
    feed: &CameraFeed,
    snapshots: &Snapshots,
) {
    draw_picture(canvas, fonts, kit, area, s, snapshots.get(&feed.snapshot));
}

/// `image` letterboxed in `area`, keeping its shape, or a note while it's
/// on the way.
pub fn draw_picture(canvas: &mut Canvas, fonts: &mut Fonts, kit: &Kit, area: Card, s: f32, image: Option<&Image>) {
    match image.filter(|i| i.is_valid()) {
        Some(image) => {
            let scale = (area.w / image.width as f32).min(area.h / image.height as f32);
            let (w, h) = (image.width as f32 * scale, image.height as f32 * scale);
            canvas.draw_image(image, area.x + (area.w - w) / 2.0, area.y + (area.h - h) / 2.0, w, h);
        }
        None => {
            let style = TextStyle::new(Face::Medium, 30.0 * s, kit.p.muted).align(Align::Center);
            canvas.text(fonts, area.x + area.w / 2.0, area.y + area.h / 2.0, style, "Waiting for a picture");
        }
    }
}

/// One dot per camera at the top right, the one that's up lit.
fn dots(canvas: &mut Canvas, kit: &Kit, card: Card, s: f32, count: usize, up: usize) {
    let (r, gap) = (6.0 * s, 20.0 * s);
    let right = card.x + card.w - 40.0 * s;
    let cy = card.y + 50.0 * s;
    for i in 0..count {
        let cx = right - r - (count - 1 - i) as f32 * gap;
        let color = if i == up { kit.p.accent } else { kit.p.muted };
        canvas.fill_round_rect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use marqueet_core::theme::Theme;

    fn view(n: usize) -> CameraView {
        let feeds = (0..n).map(|i| CameraFeed { title: format!("CAM {i}"), snapshot: format!("camera:camera.c{i}") });
        CameraView { feeds: feeds.collect(), turn_secs: 10 }
    }

    #[test]
    fn cameras_take_turns() {
        assert_eq!(turn(&view(1), 95), 0);
        assert_eq!([0, 9, 10, 25, 30].map(|t| turn(&view(3), t)), [0, 0, 1, 2, 0]);
        assert_eq!(turn(&CameraView { turn_secs: 0, ..view(2) }, 3), 1, "no divide by zero");
    }

    #[test]
    fn draws_the_picture_once_it_arrives() {
        let (mut c, mut fonts) = (Canvas::new(640, 600), Fonts::new());
        let card = Card { x: 0.0, y: 0.0, w: 640.0, h: 600.0 };
        let kit = Kit::new(&Theme::default());
        let red = Image::new(2, 2, [250, 10, 10, 255].repeat(4)).unwrap();
        let is_red = |c: &Canvas| c.pixel(320, 350)[..3] == [250, 10, 10];
        draw(&mut c, &mut fonts, &kit, card, 1.0, &view(2), &Snapshots::new(), 0);
        assert!(!is_red(&c), "nothing yet");
        let snapshots: Snapshots = [("camera:camera.c1".to_owned(), red)].into();
        draw(&mut c, &mut fonts, &kit, card, 1.0, &view(2), &snapshots, 0);
        assert!(!is_red(&c), "another camera is up");
        draw(&mut c, &mut fonts, &kit, card, 1.0, &view(2), &snapshots, 10);
        assert!(is_red(&c), "its turn");
    }
}
