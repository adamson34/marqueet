//! Headless rendering to PNG files: single frames for previewing resolutions
//! and checking the LED look without a monitor, or frame sequences for demo
//! videos (`ffmpeg -i frame_%05d.png ...`).

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use chrono::{Local, Utc};
use marqueet_core::config::DisplayConfig;
use marqueet_core::sports::HomeAway;

use crate::render::{self, Renderer};
use crate::scene::{FeedSource, Scene, SceneSetup};

#[derive(Debug)]
pub enum Output {
    Frame(PathBuf),
    Frames { dir: PathBuf, duration: f64, fps: u32 },
}

#[derive(Debug)]
pub struct Options {
    pub output: Output,
    pub size: (u32, u32),
    /// Simulated seconds before the (first) capture.
    pub at: f64,
    pub source: FeedSource,
    pub scroll_to: Option<String>,
    pub flash: Option<String>,
    /// Simulated second at which `flash` starts.
    pub flash_at: f64,
    /// Score points for a mock game (and flash it) at `score_at`.
    pub score: Option<(String, HomeAway, u16)>,
    pub score_at: f64,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// An offscreen render target plus readback buffer.
struct Headless {
    device: wgpu::Device,
    queue: wgpu::Queue,
    texture: wgpu::Texture,
    buffer: wgpu::Buffer,
    renderer: Renderer,
    size: (u32, u32),
    padded_row: u32,
}

impl Headless {
    fn new(size: (u32, u32)) -> render::Result<Self> {
        let (w, h) = size;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))?;
        let (device, queue) = pollster::block_on(render::request_device(&adapter))?;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        // Buffer copies need rows padded to 256 bytes.
        let padded_row = (w * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("headless readback"),
            size: u64::from(padded_row * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let renderer = Renderer::new(&device, FORMAT);
        Ok(Headless { device, queue, texture, buffer, renderer, size, padded_row })
    }

    /// Renders the scene and returns tightly packed RGBA8 (sRGB) pixels.
    fn capture(&mut self, scene: &mut Scene) -> render::Result<Vec<u8>> {
        let (w, h) = self.size;
        let view = self.texture.create_view(&Default::default());
        self.renderer.render(&self.device, &self.queue, scene, &view);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        self.buffer.map_async(wgpu::MapMode::Read, .., |r| {
            if let Err(e) = r {
                log::error!("readback failed: {e}");
            }
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        let row_bytes = (w * 4) as usize;
        let mut pixels = Vec::with_capacity(row_bytes * h as usize);
        {
            let mapped = self.buffer.get_mapped_range(..)?;
            for row in mapped.chunks(self.padded_row as usize) {
                pixels.extend_from_slice(&row[..row_bytes]);
            }
        }
        self.buffer.unmap();
        Ok(pixels)
    }
}

fn write_png(path: &Path, (w, h): (u32, u32), pixels: &[u8]) -> render::Result<()> {
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    writer.finish()?;
    Ok(())
}

pub fn run(config: DisplayConfig, opts: Options) -> render::Result<()> {
    let mut headless = Headless::new(opts.size)?;
    let now = Utc::now();
    let mut scene = Scene::new(
        config.clone(),
        opts.size.0,
        opts.size.1,
        SceneSetup {
            now,
            tz: *Local::now().offset(),
            source: opts.source.clone(),
            max_strip_width: Renderer::max_strip_width(config.ticker_rows.max(config.crawl_rows)),
        },
    );

    // Simulate at 60 fps so frames match what the window would show.
    const DT: f64 = 1.0 / 60.0;
    let (mut flashed, mut scored) = (false, false);
    let mut advance_to = |scene: &mut Scene, t: f64| {
        while scene.time + DT / 2.0 < t {
            if let (Some(id), false) = (&opts.flash, flashed)
                && scene.time >= opts.flash_at
            {
                scene.flash_ticker(id);
                flashed = true;
            }
            if let (Some((id, side, points)), false) = (&opts.score, scored)
                && scene.time >= opts.score_at
            {
                if !scene.score(id, *side, *points) {
                    log::warn!("--score: no mock game with id {id:?}");
                }
                scored = true;
            }
            scene.update(DT, now);
        }
    };

    // Live mode: wait (in real time) for the server's first content.
    if !scene.has_content() {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !scene.has_content() {
            if std::time::Instant::now() > deadline {
                return Err("no content from marqueet-server within 10 s (is it running? or use --mock)".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
            scene.update(0.0, now);
        }
    }
    advance_to(&mut scene, opts.at);
    if let Some(id) = &opts.scroll_to
        && !scene.scroll_ticker_to(id)
    {
        return Err(format!("no ticker segment with id {id:?}").into());
    }

    match &opts.output {
        Output::Frame(path) => {
            let pixels = headless.capture(&mut scene)?;
            write_png(path, opts.size, &pixels)?;
            log::info!("wrote {}", path.display());
        }
        Output::Frames { dir, duration, fps } => {
            std::fs::create_dir_all(dir)?;
            let frames = (duration * f64::from(*fps)).round() as u32;
            for i in 0..frames {
                advance_to(&mut scene, opts.at + f64::from(i) / f64::from(*fps));
                let pixels = headless.capture(&mut scene)?;
                write_png(&dir.join(format!("frame_{i:05}.png")), opts.size, &pixels)?;
            }
            log::info!("wrote {frames} frames to {}", dir.display());
        }
    }
    Ok(())
}
