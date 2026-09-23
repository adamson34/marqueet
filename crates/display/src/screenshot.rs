//! Headless rendering to a PNG: for previewing resolutions and for checking
//! the LED look without a monitor.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use chrono::{Local, Utc};
use tickadee_core::config::DisplayConfig;

use crate::render::{self, Renderer};
use crate::scene::Scene;

#[derive(Debug)]
pub struct Options {
    pub path: PathBuf,
    pub size: (u32, u32),
    pub at: f64,
    pub seed: u64,
    pub scroll_to: Option<String>,
    pub flash: Option<String>,
    pub flash_age: f64,
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub fn run(config: DisplayConfig, opts: Options) -> render::Result<()> {
    let (w, h) = opts.size;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
        apply_limit_buckets: false,
    }))?;
    let (device, queue) = pollster::block_on(render::request_device(&adapter))?;

    let now = Utc::now();
    let mut scene = Scene::new(
        config.clone(),
        w,
        h,
        now,
        *Local::now().offset(),
        opts.seed,
        Renderer::max_strip_width(config.ticker_rows.max(config.crawl_rows)),
    );
    // Simulate at 60 fps so the result matches what the window would show.
    let dt = 1.0 / 60.0;
    let flash_at = opts.at - opts.flash_age;
    let mut flashed = false;
    while scene.time + dt / 2.0 < opts.at {
        if let (Some(id), false) = (&opts.flash, flashed)
            && scene.time >= flash_at
        {
            scene.flash_ticker(id);
            flashed = true;
        }
        scene.update(dt, now);
        if let Some(id) = &opts.scroll_to
            && !scene.scroll_ticker_to(id)
        {
            return Err(format!("no ticker segment with id {id:?}").into());
        }
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = Renderer::new(&device, FORMAT);
    renderer.render(&device, &queue, &mut scene, &texture.create_view(&Default::default()));

    // Copy out; rows must be padded to 256 bytes.
    let row_bytes = w * 4;
    let padded = row_bytes.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot readback"),
        size: u64::from(padded * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |r| {
        if let Err(e) = r {
            log::error!("readback failed: {e}");
        }
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    let mapped = buffer.get_mapped_range(..)?;
    let mut pixels = Vec::with_capacity((row_bytes * h) as usize);
    for row in mapped.chunks(padded as usize) {
        pixels.extend_from_slice(&row[..row_bytes as usize]);
    }
    drop(mapped);
    buffer.unmap();

    let mut encoder = png::Encoder::new(BufWriter::new(File::create(&opts.path)?), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&pixels)?;
    writer.finish()?;
    log::info!("wrote {}", opts.path.display());
    Ok(())
}
