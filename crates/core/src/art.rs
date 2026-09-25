//! Fans' LED art for their team's takeovers: a still picture or a short
//! animation, one LED per pixel. Pure: uploaded bytes (a PNG, a PNG sprite
//! strip or an animated GIF) in, frames out; no I/O. Nothing ships with
//! Marqueet (ADR-0013): people draw or bring their own.

use std::io::Cursor;

use serde::{Deserialize, Serialize};

use crate::color::Rgb;
use crate::team_art::Image;
use crate::ticker::LedBitmap;

/// Widest art, in LEDs (one per pixel).
pub const ART_MAX_W: u32 = 96;
/// Tallest art, in LEDs.
pub const ART_MAX_H: u32 = 48;
/// Most frames in an animation.
pub const ART_MAX_FRAMES: usize = 48;
/// Largest upload.
pub const ART_MAX_BYTES: usize = 1024 * 1024;
/// Frame time bounds, in milliseconds.
pub const FRAME_MS_MIN: u16 = 40;
pub const FRAME_MS_MAX: u16 = 2000;
/// Frame time when the file doesn't say.
pub const FRAME_MS_DEFAULT: u16 = 150;

/// Where the art goes in a takeover.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtPlacement {
    /// Above the headline, for the whole takeover.
    #[default]
    Above,
    /// On its own, big, for the first few seconds; then the words.
    Intro,
}

impl ArtPlacement {
    pub fn id(self) -> &'static str {
        match self {
            ArtPlacement::Above => "above",
            ArtPlacement::Intro => "intro",
        }
    }

    pub fn from_id(id: &str) -> Option<ArtPlacement> {
        [ArtPlacement::Above, ArtPlacement::Intro].into_iter().find(|p| p.id() == id)
    }
}

/// A team's takeover art: one frame is a still; more play in a loop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TakeoverArt {
    /// All the same size, at most [`ART_MAX_W`] x [`ART_MAX_H`].
    pub frames: Vec<Image>,
    /// How long each frame shows.
    pub frame_ms: u16,
    #[serde(default)]
    pub placement: ArtPlacement,
}

impl TakeoverArt {
    /// Frames checked: at least one, all the same size and within the limits.
    pub fn new(frames: Vec<Image>, frame_ms: u16, placement: ArtPlacement) -> Result<TakeoverArt, String> {
        let first = frames.first().ok_or("that file has no pictures in it")?;
        let (w, h) = (first.width, first.height);
        if w > ART_MAX_W || h > ART_MAX_H {
            return Err(format!(
                "LED art is one light per pixel, so it can be at most {ART_MAX_W}x{ART_MAX_H}; that one is {w}x{h}"
            ));
        }
        if frames.len() > ART_MAX_FRAMES {
            return Err(format!("that animation has {} frames; {ART_MAX_FRAMES} at most", frames.len()));
        }
        if frames.iter().any(|f| !f.is_valid() || f.width != w || f.height != h) {
            return Err("every frame has to be the same size".into());
        }
        Ok(TakeoverArt { frames, frame_ms: frame_ms.clamp(FRAME_MS_MIN, FRAME_MS_MAX), placement })
    }

    /// True for art from outside (a team pack, the database) that keeps the
    /// rules [`TakeoverArt::new`] checks.
    pub fn is_valid(&self) -> bool {
        TakeoverArt::new(self.frames.clone(), self.frame_ms, self.placement).is_ok_and(|a| a.frame_ms == self.frame_ms)
    }

    pub fn size(&self) -> (u32, u32) {
        self.frames.first().map_or((0, 0), |f| (f.width, f.height))
    }

    /// The frame showing `secs` after the takeover started (looping).
    pub fn frame_at(&self, secs: f64) -> usize {
        if self.frames.len() < 2 {
            return 0;
        }
        let n = (secs.max(0.0) * 1000.0 / f64::from(self.frame_ms)) as usize;
        n % self.frames.len()
    }

    /// One loop of the animation, in seconds.
    pub fn loop_secs(&self) -> f64 {
        self.frames.len() as f64 * f64::from(self.frame_ms) / 1000.0
    }
}

impl Image {
    /// One LED per pixel: lit where the pixel is mostly opaque and not
    /// near-black (an LED can't show black).
    pub fn led_bitmap(&self) -> LedBitmap {
        let mut out = LedBitmap::new(self.width, self.height);
        for y in 0..self.height {
            for x in 0..self.width {
                let i = ((y * self.width + x) * 4) as usize;
                let [r, g, b, a] = [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]];
                let c = Rgb::new(r, g, b);
                if a >= 128 && c.luminance() > 0.01 {
                    out.set(x, y, c);
                }
            }
        }
        out
    }
}

/// Frames from an upload: an animated GIF (its own timing), a PNG, or a PNG
/// sprite strip holding `strip_frames` frames side by side. Returns the
/// frames and the file's frame time, if it has one.
pub fn decode(bytes: &[u8], strip_frames: u32) -> Result<(Vec<Image>, Option<u16>), String> {
    if bytes.len() > ART_MAX_BYTES {
        return Err("that file is too big for LED art; keep it under 1 MB".into());
    }
    if bytes.starts_with(b"GIF8") {
        return decode_gif(bytes);
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("LED art has to be a PNG or a GIF".into());
    }
    let image = decode_png(bytes)?;
    let n = strip_frames.max(1);
    if n == 1 {
        check_size(image.width, image.height)?;
        return Ok((vec![image], None));
    }
    if image.width % n != 0 {
        return Err(format!("that strip is {} pixels wide, which doesn't split into {n} frames", image.width));
    }
    let fw = image.width / n;
    check_size(fw, image.height)?;
    let frames = (0..n)
        .map(|f| {
            let mut rgba = Vec::with_capacity((fw * image.height * 4) as usize);
            for y in 0..image.height {
                let start = ((y * image.width + f * fw) * 4) as usize;
                rgba.extend_from_slice(&image.rgba[start..start + (fw * 4) as usize]);
            }
            Image::new(fw, image.height, rgba).ok_or_else(|| "couldn't split that strip".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((frames, None))
}

/// One LED per pixel: a frame bigger than the limit won't fit.
fn check_size(w: u32, h: u32) -> Result<(), String> {
    if w > ART_MAX_W || h > ART_MAX_H {
        return Err(format!(
            "LED art is one light per pixel, so it can be at most {ART_MAX_W}x{ART_MAX_H}; that one is {w}x{h}"
        ));
    }
    Ok(())
}

fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let bad = |e: png::DecodingError| format!("couldn't read that PNG ({e})");
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(bad)?;
    let (w, h) = (reader.info().width, reader.info().height);
    // A strip can be wide; the frame size is checked after splitting.
    if w == 0 || h == 0 || w > ART_MAX_W * ART_MAX_FRAMES as u32 || h > ART_MAX_H {
        return Err(format!(
            "LED art is one light per pixel, so it can be at most {ART_MAX_W}x{ART_MAX_H}; that one is {w}x{h}"
        ));
    }
    let mut buf = vec![0; reader.output_buffer_size().ok_or("that PNG is too large")?];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    let data = &buf[..frame.buffer_size()];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("couldn't read that PNG's colors".into()),
    };
    Image::new(frame.width, frame.height, rgba).ok_or_else(|| "couldn't read that PNG".into())
}

fn decode_gif(bytes: &[u8]) -> Result<(Vec<Image>, Option<u16>), String> {
    let bad = |e: gif::DecodingError| format!("couldn't read that GIF ({e})");
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(Cursor::new(bytes)).map_err(bad)?;
    let (w, h) = (u32::from(decoder.width()), u32::from(decoder.height()));
    if w == 0 || h == 0 || w > ART_MAX_W || h > ART_MAX_H {
        return Err(format!(
            "LED art is one light per pixel, so it can be at most {ART_MAX_W}x{ART_MAX_H}; that one is {w}x{h}"
        ));
    }
    let mut canvas = vec![0u8; (w * h * 4) as usize];
    let (mut frames, mut delay) = (Vec::new(), None);
    while let Some(frame) = decoder.read_next_frame().map_err(bad)? {
        if frames.len() == ART_MAX_FRAMES {
            return Err(format!("that animation has more than {ART_MAX_FRAMES} frames"));
        }
        if delay.is_none() && frame.delay > 0 {
            delay = Some(frame.delay.saturating_mul(10));
        }
        let before = canvas.clone();
        let (fx, fy) = (u32::from(frame.left), u32::from(frame.top));
        let (fw, fh) = (u32::from(frame.width), u32::from(frame.height));
        for y in 0..fh {
            for x in 0..fw {
                let (cx, cy) = (fx + x, fy + y);
                let s = ((y * fw + x) * 4) as usize;
                if cx >= w || cy >= h || s + 4 > frame.buffer.len() || frame.buffer[s + 3] == 0 {
                    continue;
                }
                let d = ((cy * w + cx) * 4) as usize;
                canvas[d..d + 4].copy_from_slice(&frame.buffer[s..s + 4]);
            }
        }
        frames.push(Image::new(w, h, canvas.clone()).ok_or("couldn't read that GIF")?);
        match frame.dispose {
            gif::DisposalMethod::Background => {
                for y in fy..(fy + fh).min(h) {
                    for x in fx..(fx + fw).min(w) {
                        let d = ((y * w + x) * 4) as usize;
                        canvas[d..d + 4].fill(0);
                    }
                }
            }
            gif::DisposalMethod::Previous => canvas = before,
            _ => {}
        }
    }
    Ok((frames, delay))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` x `h` PNG of `frames` frames side by side, frame `i` lit only
    /// in column `i`.
    fn strip_png(w: u32, h: u32, frames: u32) -> Vec<u8> {
        let mut rgba = vec![0u8; (w * frames * h * 4) as usize];
        for f in 0..frames {
            for y in 0..h {
                let i = ((y * w * frames + f * w + f % w) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[255, 200, 0, 255]);
            }
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w * frames, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
        out
    }

    fn gif_bytes(w: u16, h: u16, frames: usize, delay: u16) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let palette = [0u8, 0, 0, 255, 40, 40];
            let mut enc = gif::Encoder::new(&mut out, w, h, &palette).unwrap();
            for f in 0..frames {
                let mut px = vec![0u8; usize::from(w) * usize::from(h)];
                let n = px.len();
                px[f % n] = 1;
                let mut frame = gif::Frame::from_indexed_pixels(w, h, px, None);
                frame.delay = delay;
                enc.write_frame(&frame).unwrap();
            }
        }
        out
    }

    #[test]
    fn a_png_is_one_frame_and_a_strip_splits() {
        let (one, ms) = decode(&strip_png(16, 8, 1), 1).unwrap();
        assert_eq!((one.len(), one[0].width, ms), (1, 16, None));
        let (four, _) = decode(&strip_png(16, 8, 4), 4).unwrap();
        assert_eq!(four.len(), 4);
        assert!(four.iter().all(|f| (f.width, f.height) == (16, 8)));
        assert!(
            four[2].led_bitmap().get(2, 0).is_some() && four[2].led_bitmap().get(0, 0).is_none(),
            "each frame its own"
        );
        assert!(decode(&strip_png(16, 8, 4), 3).unwrap_err().contains("doesn't split"));
    }

    #[test]
    fn a_gif_keeps_its_frames_and_timing() {
        let (frames, ms) = decode(&gif_bytes(12, 6, 5, 8), 1).unwrap();
        assert_eq!((frames.len(), ms), (5, Some(80)));
        let art = TakeoverArt::new(frames, ms.unwrap(), ArtPlacement::Intro).unwrap();
        assert_eq!(art.frame_at(0.0), 0);
        assert_eq!(art.frame_at(0.17), 2);
        assert_eq!(art.frame_at(0.41), 0, "loops");
        assert!(art.is_valid());
    }

    #[test]
    fn limits_say_what_to_change() {
        assert!(decode(&strip_png(120, 8, 1), 1).unwrap_err().contains("at most 96x48"));
        assert!(decode(&gif_bytes(100, 10, 1, 5), 1).unwrap_err().contains("at most 96x48"));
        assert!(decode(b"not an image", 1).unwrap_err().contains("PNG or a GIF"));
        let (frames, _) = decode(&strip_png(8, 8, 1), 1).unwrap();
        let art = TakeoverArt::new(frames, 5, ArtPlacement::Above).unwrap();
        assert_eq!(art.frame_ms, FRAME_MS_MIN, "too fast is slowed to the minimum");
        assert!(TakeoverArt::new(Vec::new(), 100, ArtPlacement::Above).is_err());
    }

    #[test]
    fn black_and_see_through_pixels_stay_dark() {
        let rgba = vec![0, 0, 0, 255, 255, 0, 0, 40, 255, 0, 0, 255];
        let led = Image::new(3, 1, rgba).unwrap().led_bitmap();
        assert_eq!((led.get(0, 0), led.get(1, 0)), (None, None));
        assert_eq!(led.get(2, 0), Some(Rgb::new(255, 0, 0)));
    }
}
