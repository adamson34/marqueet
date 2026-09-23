//! Draws a [`Scene`] with wgpu: device setup, per-frame uploads and passes.

use tickadee_core::Rgb;
use tickadee_core::config::ScrollMode;

use crate::band::Upload;
use crate::gpu::{LedPipelines, MAX_TEX, PanelGpu, Params, STRIP_TILE_W};
use crate::scene::Scene;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Widget-area background, sRGB.
const PAGE_BG: Rgb = Rgb::new(10, 11, 14);
/// LED band background, sRGB.
const BAND_BG: Rgb = Rgb::new(3, 3, 4);

fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear(c: Rgb, a: f32) -> [f32; 4] {
    [srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b), a]
}

pub async fn request_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue)> {
    let info = adapter.get_info();
    log::info!("GPU: {} ({:?}, {:?})", info.name, info.backend, info.device_type);
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("tickadee"),
            required_features: wgpu::Features::empty(),
            // Only ask for what a WebGL2-class GPU (e.g. Raspberry Pi 4) offers.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })
        .await?;
    Ok((device, queue))
}

/// GPU renderer for a scene. One [`PanelGpu`] per scene panel.
#[derive(Debug)]
pub struct Renderer {
    pipes: LedPipelines,
    panels: Vec<PanelGpu>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Self {
        Renderer { pipes: LedPipelines::new(device, output_format), panels: Vec::new() }
    }

    /// Longest strip a panel can hold; the scene truncates beyond this.
    pub fn max_strip_width(rows: u32) -> u32 {
        (MAX_TEX / rows.max(1)).max(1) * STRIP_TILE_W
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &mut Scene,
        target: &wgpu::TextureView,
    ) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        let cfg = scene.config.clone();

        // Recreate panel resources if the layout changed.
        let stale = self.panels.len() != scene.panels.len()
            || self.panels.iter().zip(&scene.panels).any(|(g, p)| g.dims() != (p.grid.cols, p.grid.rows));
        if stale {
            self.panels = scene.panels.iter().map(|p| PanelGpu::new(device, p.grid.cols, p.grid.rows)).collect();
            for p in &mut scene.panels {
                p.band.uploads.insert(0, Upload::Full);
            }
        }

        for (gpu, panel) in self.panels.iter_mut().zip(&mut scene.panels) {
            for upload in panel.band.take_uploads() {
                match upload {
                    Upload::Full => gpu.upload_strip(device, queue, &panel.band.strip.bitmap),
                    Upload::Columns { start, bitmap } => gpu.upload_columns(queue, &bitmap, start),
                }
            }
            let g = panel.grid;
            let (base, frac) = panel.band.scroll();
            let smooth = panel.band.wrap && cfg.scroll_mode == ScrollMode::Smooth;
            let unlit = Rgb::new(22, 22, 24).mix(cfg.led_color, 0.08);
            let params = Params {
                origin_pitch: [g.origin.0 as f32, g.origin.1 as f32, g.pitch as f32, cfg.dot_size],
                grid: [g.cols as f32, g.rows as f32, panel.band.strip.width() as f32, STRIP_TILE_W as f32],
                scroll: [base as f32, frac, f32::from(u8::from(smooth)), f32::from(u8::from(panel.band.wrap))],
                look: [cfg.glow, cfg.flicker, (scene.time % 3600.0) as f32, 1.0],
                off_color: linear(unlit, 0.0),
                bg: linear(BAND_BG, f32::from(u8::from(self.pipes.manual_srgb))),
                blur: [0.0; 4],
            };
            gpu.prepare(device, queue, &self.pipes, &mut encoder, &params);
        }

        {
            let bg = linear(PAGE_BG, 1.0);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(if self.pipes.manual_srgb {
                            wgpu::Color {
                                r: f64::from(PAGE_BG.r) / 255.0,
                                g: f64::from(PAGE_BG.g) / 255.0,
                                b: f64::from(PAGE_BG.b) / 255.0,
                                a: 1.0,
                            }
                        } else {
                            wgpu::Color { r: f64::from(bg[0]), g: f64::from(bg[1]), b: f64::from(bg[2]), a: 1.0 }
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for (gpu, panel) in self.panels.iter_mut().zip(&scene.panels) {
                let b = panel.grid.band;
                gpu.composite(device, &self.pipes, &mut pass, [b.x, b.y, b.w, b.h]);
            }
        }
        queue.submit([encoder.finish()]);
    }
}
