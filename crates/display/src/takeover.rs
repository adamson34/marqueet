//! Takeovers: queueing, timing, colors and layout. Pure (no GPU).
//!
//! A takeover replaces the widget area for about ten seconds when a big play
//! happens (ADR-0007): a background in the look's style (ADR-0012), a small
//! kicker line, a big LED-block headline, the play, the score, and an optional
//! fantasy note, all as LED blocks from the project's font.
//!
//! - Broadcast: the scoring team's color in drifting diagonal stripes.
//! - Ballpark: the painted green board in vertical planks, yellow bulbs.
//! - Varsity: the team's jersey color with an athletic mesh.

use std::collections::VecDeque;

use marqueet_core::Rgb;
use marqueet_core::alert::{Alert, AlertLevel, Takeover};
use marqueet_core::font::BitmapFont;
use marqueet_core::layout::{LedGrid, Rect};
use marqueet_core::theme::{Style, Theme, contrast};
use marqueet_core::ticker::{Part, Span, TickerSegment, Tint};

/// How long one takeover is on screen, including fades.
pub const DURATION: f64 = 10.0;
pub const FADE: f64 = 0.35;
/// Pause between back-to-back takeovers.
const GAP: f64 = 0.8;
/// Queued takeovers older than this are dropped: stale news.
const MAX_AGE: f64 = 45.0;
const MAX_QUEUED: usize = 4;

const KICKER: Rgb = Rgb::new(0xfd, 0xe3, 0xb8);
const MUTED: Rgb = Rgb::new(0xb9, 0xc3, 0xda);

#[derive(Clone, Debug, PartialEq)]
pub struct Active {
    pub alert: Alert,
    pub takeover: Takeover,
    pub started: f64,
}

/// Plays queued takeovers one at a time.
#[derive(Debug, Default)]
pub struct Queue {
    waiting: VecDeque<(Alert, f64)>,
    active: Option<Active>,
    /// Earliest time the next takeover may start.
    next_start: f64,
}

impl Queue {
    /// Queues a takeover alert (other levels and alerts without takeover
    /// details are ignored). Drops the oldest when the queue is full.
    pub fn push(&mut self, alert: Alert, now: f64) {
        if alert.level != AlertLevel::Takeover || alert.takeover.is_none() {
            return;
        }
        let duplicate = self.active.as_ref().is_some_and(|a| a.alert.id == alert.id)
            || self.waiting.iter().any(|(a, _)| a.id == alert.id);
        if duplicate {
            return;
        }
        if self.waiting.len() == MAX_QUEUED {
            self.waiting.pop_front();
        }
        self.waiting.push_back((alert, now));
    }

    /// Advances to `now`. Returns true when the active takeover changed
    /// (started, ended, or was replaced) so the scene can rebuild panels.
    pub fn update(&mut self, now: f64) -> bool {
        let mut changed = false;
        if self.active.as_ref().is_some_and(|a| now - a.started >= DURATION) {
            self.active = None;
            self.next_start = now + GAP;
            changed = true;
        }
        self.waiting.retain(|(_, queued)| now - queued <= MAX_AGE);
        if self.active.is_none()
            && now >= self.next_start
            && let Some((alert, _)) = self.waiting.pop_front()
        {
            let takeover = alert.takeover.clone().unwrap_or_else(|| unreachable_takeover(&alert));
            self.active = Some(Active { alert, takeover, started: now });
            changed = true;
        }
        changed
    }

    pub fn active(&self) -> Option<&Active> {
        self.active.as_ref()
    }

    /// 0..1 fade for the active takeover.
    pub fn opacity(&self, now: f64) -> f32 {
        let Some(a) = &self.active else { return 0.0 };
        let t = now - a.started;
        let fade_in = (t / FADE).clamp(0.0, 1.0);
        let fade_out = ((DURATION - t) / FADE).clamp(0.0, 1.0);
        fade_in.min(fade_out) as f32
    }
}

/// `push` only accepts alerts with takeover details, so this never runs; it
/// keeps `update` free of panics.
fn unreachable_takeover(alert: &Alert) -> Takeover {
    Takeover { kicker: String::new(), headline: alert.title.clone(), play: None, score: None, note: None }
}

/// How the background is patterned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pattern {
    /// Direction the stripes run across (unit vector).
    pub dir: (f32, f32),
    /// Stripe period, px at 1920 wide.
    pub width: f32,
    /// Drift, px per second at 1920 wide (0 holds still).
    pub speed: f32,
    /// Dot texture: > 0 lightens, < 0 darkens (mesh), 0 none.
    pub dots: f32,
    /// Dot spacing, px at 1920 wide.
    pub cell: f32,
}

/// A takeover's colors: background, boxes, bottom bar and each text line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub stripe_a: Rgb,
    pub stripe_b: Rgb,
    pub box_fill: Rgb,
    pub pill: Rgb,
    pub bar: Rgb,
    pub pattern: Pattern,
    pub kicker: Rgb,
    pub headline: Rgb,
    pub play: Rgb,
    /// The scoring team's side of the score line.
    pub scorer: Rgb,
    pub other: Rgb,
    /// Text on the note pill.
    pub note: Rgb,
}

/// A team color, kept in hue but dark enough for light text on top.
pub fn team_base(primary: Rgb) -> Rgb {
    let mut base = primary;
    while base.luminance() > 0.06 {
        base = base.scale(0.85);
    }
    if base.luminance() < 0.004 {
        // Near-black teams get a charcoal so the stripes still show.
        base = base.mix(Rgb::new(0x30, 0x32, 0x3a), 0.6);
    }
    base
}

impl Palette {
    /// Colors for a takeover in `theme`, for a play by a team with `colors`
    /// (primary, secondary) when the alert carries them.
    pub fn new(theme: &Theme, colors: Option<(Rgb, Rgb)>, fallback: Rgb) -> Palette {
        let p = &theme.palette;
        let (primary, secondary) = colors.filter(|_| theme.team_colors).unwrap_or((fallback, p.accent));
        let diagonal = Pattern { dir: (0.819, 0.574), width: 120.0, speed: 24.0, dots: 0.06, cell: 14.0 };
        match theme.style {
            Style::Broadcast => {
                let base = if theme.team_colors { team_base(primary) } else { team_base(p.panel) };
                Palette {
                    stripe_a: base,
                    stripe_b: base.mix(Rgb::WHITE, 0.10),
                    box_fill: p.plate,
                    pill: p.strip,
                    bar: p.accent,
                    pattern: diagonal,
                    kicker: KICKER,
                    headline: Rgb::WHITE,
                    play: Rgb::WHITE,
                    scorer: p.accent,
                    other: MUTED,
                    note: p.strip_text,
                }
            }
            Style::Ballpark => Palette {
                stripe_a: p.panel,
                stripe_b: p.panel.scale(0.9),
                box_fill: p.plate,
                pill: p.plate,
                bar: p.accent,
                pattern: Pattern { dir: (1.0, 0.0), width: 96.0, speed: 0.0, dots: 0.0, cell: 14.0 },
                kicker: p.accent,
                // Bulb yellow, like a scoreboard's message board.
                headline: p.accent.mix(Rgb::WHITE, 0.25),
                play: p.text,
                scorer: p.accent,
                other: p.muted,
                note: p.text,
            },
            Style::Varsity => {
                let base = if theme.team_colors { team_base(primary) } else { team_base(p.panel) };
                let trim = if contrast(secondary, base) >= 2.0 { secondary } else { p.accent };
                Palette {
                    stripe_a: base,
                    stripe_b: base,
                    box_fill: p.plate,
                    pill: p.text,
                    bar: trim,
                    pattern: Pattern { dir: (1.0, 0.0), width: 120.0, speed: 0.0, dots: -0.22, cell: 10.0 },
                    kicker: p.text,
                    headline: Rgb::WHITE,
                    play: p.text,
                    scorer: p.accent,
                    other: p.muted,
                    note: p.plate,
                }
            }
        }
    }
}

/// Where everything goes, in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub area: Rect,
    pub kicker: LedGrid,
    pub headline: LedGrid,
    pub play: Option<LedGrid>,
    pub score: Option<LedGrid>,
    pub score_box: Option<Rect>,
    pub note: Option<LedGrid>,
    pub note_pill: Option<Rect>,
}

/// Rows of a small-font line (7-row capitals + margins).
const SMALL_ROWS: u32 = 11;
/// Rows of the headline (Scale2x capitals + margins).
const BIG_ROWS: u32 = 18;

fn small_width(text: &str) -> u32 {
    BitmapFont::small().text_width(text)
}

/// Grid for a centered line of `cols` x `rows` LEDs at `pitch`, top at `y`.
fn line(area: Rect, cols: u32, rows: u32, pitch: u32, y: u32) -> LedGrid {
    let pitch = pitch.max(1);
    let cols = cols.min(area.w / pitch).max(1);
    let w = cols * pitch;
    let x = area.x + (area.w - w) / 2;
    LedGrid { band: Rect { x, y, w, h: rows * pitch }, origin: (x, y), pitch, cols, rows }
}

pub fn layout(area: Rect, t: &Takeover) -> Layout {
    let big = BitmapFont::large();
    let head_cols = big.text_width(&t.headline) + 8;
    let head_pitch = ((area.w as f32 * 0.88) / head_cols as f32).min(area.h as f32 * 0.34 / BIG_ROWS as f32) as u32;
    let head_pitch = head_pitch.max(2);
    let small_pitch = ((head_pitch as f32 * 0.42).round() as u32).max(2);
    let score_pitch = ((head_pitch as f32 * 0.5).round() as u32).max(2);

    let score_text = score_text(t);
    let lines_h = SMALL_ROWS * small_pitch                      // kicker
        + BIG_ROWS * head_pitch                                  // headline
        + t.play.as_ref().map_or(0, |_| SMALL_ROWS * small_pitch)
        + t.score.as_ref().map_or(0, |_| SMALL_ROWS * score_pitch + score_pitch * 2) // score + box padding
        + t.note.as_ref().map_or(0, |_| SMALL_ROWS * small_pitch + small_pitch * 2);
    let gaps = small_pitch * 3;
    let mut y = area.y + area.h.saturating_sub(lines_h + gaps * 3) / 2;

    let kicker = line(area, small_width(&t.kicker) + 4, SMALL_ROWS, small_pitch, y);
    y += kicker.band.h + gaps / 2;
    let headline = line(area, head_cols, BIG_ROWS, head_pitch, y);
    y += headline.band.h + gaps / 2;
    let play = t.play.as_ref().map(|p| {
        let g = line(area, small_width(p) + 4, SMALL_ROWS, small_pitch, y);
        y += g.band.h + gaps / 2;
        g
    });
    let (score, score_box) = match &score_text {
        Some(text) => {
            y += score_pitch;
            let score = line(area, small_width(text) + 6, SMALL_ROWS, score_pitch, y);
            let pad = score_pitch * 2;
            let b = score.band;
            let score_box = Rect { x: b.x - pad, y: b.y - score_pitch, w: b.w + pad * 2, h: b.h + pad };
            y += b.h + score_pitch + gaps;
            (Some(score), Some(score_box))
        }
        None => {
            y += gaps;
            (None, None)
        }
    };
    let (note, note_pill) = match &t.note {
        Some((label, value)) => {
            let text = format!("{label}  |  {value}");
            let g = line(area, small_width(&text) + 6, SMALL_ROWS, small_pitch, y);
            let pad = small_pitch * 3;
            let pill = Rect { x: g.band.x - pad, y: g.band.y, w: g.band.w + pad * 2, h: g.band.h };
            (Some(g), Some(pill))
        }
        None => (None, None),
    };
    Layout { area, kicker, headline, play, score, score_box, note, note_pill }
}

pub fn score_text(t: &Takeover) -> Option<String> {
    let s = t.score.as_ref()?;
    Some(format!("{} {}   {} {}", s.away.0, s.away.1, s.home.0, s.home.1))
}

fn seg(id: &str, spans: Vec<Span>) -> TickerSegment {
    TickerSegment { id: id.into(), parts: vec![Part::text(spans)] }
}

/// Text for each line, in the same order as [`Layout`] (kicker, headline,
/// play, score, note).
pub fn segments(t: &Takeover, colors: &Palette) -> Vec<(&'static str, TickerSegment)> {
    let mut out = vec![
        ("kicker", seg("takeover:kicker", vec![Span::new(t.kicker.clone(), Tint::Color(colors.kicker))])),
        ("headline", seg("takeover:headline", vec![Span::new(t.headline.clone(), Tint::Color(colors.headline))])),
    ];
    if let Some(play) = &t.play {
        out.push(("play", seg("takeover:play", vec![Span::new(play.clone(), Tint::Color(colors.play))])));
    }
    if let Some(score) = &t.score {
        let (scorer, other) = (Tint::Color(colors.scorer), Tint::Color(colors.other));
        let (away_tint, home_tint) = if score.scoring_home { (other, scorer) } else { (scorer, other) };
        out.push((
            "score",
            seg(
                "takeover:score",
                vec![
                    Span::new(format!("{} {}", score.away.0, score.away.1), away_tint),
                    Span::new("   ", Tint::Dim),
                    Span::new(format!("{} {}", score.home.0, score.home.1), home_tint),
                ],
            ),
        ));
    }
    if let Some((label, value)) = &t.note {
        out.push((
            "note",
            seg(
                "takeover:note",
                vec![
                    Span::new(format!("{label}  |  "), Tint::Color(colors.note)),
                    Span::new(value.clone(), Tint::Color(colors.note)),
                ],
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use marqueet_core::alert::ScoreLine;

    fn takeover(note: bool) -> Takeover {
        Takeover {
            kicker: "BUFFALO BLIZZARD · Q3 4:12".into(),
            headline: "TOUCHDOWN".into(),
            play: Some("Rico Castellano 12 yd run".into()),
            score: Some(ScoreLine { away: ("KC".into(), 17), home: ("BUF".into(), 28), scoring_home: true }),
            note: note.then(|| ("YOUR PLAYER".into(), "R. Castellano +7.2 pts".into())),
        }
    }

    fn alert(id: &str, level: AlertLevel) -> Alert {
        Alert {
            id: id.into(),
            level,
            source: "test".into(),
            segment_id: None,
            title: "TOUCHDOWN".into(),
            detail: None,
            colors: None,
            takeover: Some(takeover(false)),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn plays_one_at_a_time_with_fades_and_a_gap() {
        let mut q = Queue::default();
        q.push(alert("a", AlertLevel::Takeover), 0.0);
        q.push(alert("b", AlertLevel::Takeover), 0.0);
        assert!(q.update(0.0));
        assert_eq!(q.active().unwrap().alert.id, "a");
        assert_eq!(q.opacity(0.0), 0.0);
        assert_eq!(q.opacity(5.0), 1.0);
        assert!(q.opacity(DURATION - 0.1) < 1.0);
        assert!(!q.update(5.0));
        assert!(q.update(DURATION), "a ends");
        assert!(q.active().is_none());
        assert!(!q.update(DURATION + 0.1), "gap before b");
        assert!(q.update(DURATION + GAP));
        assert_eq!(q.active().unwrap().alert.id, "b");
    }

    #[test]
    fn ignores_flashes_duplicates_and_stale_news() {
        let mut q = Queue::default();
        q.push(alert("flash", AlertLevel::Flash), 0.0);
        q.push(alert("a", AlertLevel::Takeover), 0.0);
        q.push(alert("a", AlertLevel::Takeover), 0.0);
        q.update(0.0);
        assert_eq!(q.active().unwrap().alert.id, "a");
        q.push(alert("old", AlertLevel::Takeover), 1.0);
        q.update(DURATION);
        q.update(60.0);
        assert!(q.active().is_none(), "queued 59 s ago: dropped");
    }

    #[test]
    fn queue_is_capped_dropping_the_oldest() {
        let mut q = Queue::default();
        for i in 0..10 {
            q.push(alert(&format!("{i}"), AlertLevel::Takeover), 0.0);
        }
        q.update(0.0);
        assert_eq!(q.active().unwrap().alert.id, "6", "0-5 dropped; 6-9 kept");
    }

    #[test]
    fn palettes_keep_text_readable() {
        for team in
            [Rgb::new(0x00, 0x33, 0x8d), Rgb::new(0xff, 0xb6, 0x12), Rgb::WHITE, Rgb::BLACK, Rgb::new(0xe3, 0x18, 0x37)]
        {
            let p = Palette::new(&Theme::default(), Some((team, Rgb::WHITE)), Rgb::AMBER);
            assert!(p.stripe_a.luminance() <= 0.06, "{team}");
            assert!(p.stripe_b.luminance() <= 0.12, "{team}");
            assert!(p.stripe_a.luminance() >= 0.004 || p.stripe_a != Rgb::BLACK, "{team}");
        }
        // Hue survives: Buffalo blue stays blue.
        let navy = team_base(Rgb::new(0x00, 0x33, 0x8d));
        assert!(navy.b > navy.r && navy.b > navy.g);
    }

    #[test]
    fn each_look_has_its_own_takeover() {
        use marqueet_core::theme::contrast;
        let red = Some((Rgb::new(0xc8, 0x10, 0x2e), Rgb::new(0xff, 0xb6, 0x12)));
        let broadcast = Palette::new(&Theme::preset(Style::Broadcast), red, Rgb::AMBER);
        let ballpark = Palette::new(&Theme::preset(Style::Ballpark), red, Rgb::AMBER);
        let varsity = Palette::new(&Theme::preset(Style::Varsity), red, Rgb::AMBER);
        assert!(broadcast.stripe_a.r > broadcast.stripe_a.b, "team red stripes");
        assert_eq!(ballpark.stripe_a, Style::Ballpark.palette().panel, "the painted board, not the team");
        assert_eq!(ballpark.pattern.speed, 0.0, "planks hold still");
        assert!(varsity.pattern.dots < 0.0, "a darker mesh");
        assert_eq!(varsity.bar, Rgb::new(0xff, 0xb6, 0x12), "trim in the team's second color");
        for (name, p) in [("broadcast", broadcast), ("ballpark", ballpark), ("varsity", varsity)] {
            assert!(contrast(p.headline, p.stripe_a) >= 4.5, "{name} headline reads");
            assert!(contrast(p.note, p.pill) >= 4.5, "{name} note reads");
        }
        let mut plain = Theme::preset(Style::Broadcast);
        plain.team_colors = false;
        assert_eq!(Palette::new(&plain, red, Rgb::AMBER).stripe_a, team_base(plain.palette.panel));
    }

    #[test]
    fn layout_fits_and_stacks_top_to_bottom() {
        for (w, h) in [(1920, 720), (1366, 512), (1024, 512), (800, 320)] {
            let area = Rect { x: 0, y: 400, w, h };
            for note in [false, true] {
                let t = takeover(note);
                let l = layout(area, &t);
                let mut prev_bottom = area.y;
                let grids = [Some(l.kicker), Some(l.headline), l.play, l.score, l.note];
                for g in grids.into_iter().flatten() {
                    assert!(g.band.y >= prev_bottom, "{w}x{h} note={note}: overlap");
                    assert!(g.band.x >= area.x && g.band.x + g.band.w <= area.x + area.w, "{w}x{h}: too wide");
                    prev_bottom = g.band.bottom();
                }
                assert!(prev_bottom <= area.bottom(), "{w}x{h} note={note}: runs off the bottom");
                assert!(l.headline.pitch > l.kicker.pitch);
            }
        }
    }

    #[test]
    fn headline_fits_its_text() {
        let t = takeover(false);
        let l = layout(Rect { x: 0, y: 0, w: 1920, h: 720 }, &t);
        assert!(l.headline.cols >= BitmapFont::large().text_width("TOUCHDOWN"));
    }

    #[test]
    fn scoring_side_is_highlighted() {
        let colors = Palette::new(&Theme::default(), None, Rgb::AMBER);
        let segs = segments(&takeover(true), &colors);
        let names: Vec<&str> = segs.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["kicker", "headline", "play", "score", "note"]);
        let Part::Text { spans } = &segs[3].1.parts[0] else { panic!() };
        assert_eq!(spans[2].tint, Tint::Color(colors.scorer), "BUF scored");
        assert_eq!(spans[0].tint, Tint::Color(colors.other));
    }
}
