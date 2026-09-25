//! Draws a [`Scene`] with wgpu: device setup, per-frame uploads and passes.

use marqueet_core::Rgb;
use marqueet_core::config::ScrollMode;

use crate::band::Upload;
use crate::gpu::{
    LedPipelines, MAX_TEX, PanelGpu, Params, STRIP_TILE_W, TakeoverGpu, TakeoverParams, UiGpu, UiPipeline, pack_tiles,
};
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
            label: Some("marqueet"),
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
    takeover: TakeoverGpu,
    ui_pipeline: UiPipeline,
    ui: Vec<UiGpu>,
    /// One black pixel, stretched over the screen to dim it (night mode);
    /// made the first time it's needed.
    shade: Option<UiGpu>,
}

/// How much night mode darkens the screen (0 = not at all, 1 = black).
const DIM: f32 = 0.85;

/// Lit square fraction for takeover LED blocks (small gaps between blocks).
const BLOCK_SIZE: f32 = 0.86;
const BLOCK_GLOW: f32 = 0.35;

fn rect4(r: marqueet_core::layout::Rect) -> [f32; 4] {
    [r.x as f32, r.y as f32, r.w as f32, r.h as f32]
}

impl Renderer {
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Self {
        Renderer {
            pipes: LedPipelines::new(device, output_format),
            panels: Vec::new(),
            takeover: TakeoverGpu::new(device, output_format),
            ui_pipeline: UiPipeline::new(device, output_format),
            ui: Vec::new(),
            shade: None,
        }
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
        if scene.screen_off {
            // Quiet hours: a black screen, nothing else drawn.
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("screen off"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            queue.submit([encoder.finish()]);
            return;
        }
        let cfg = scene.config.clone();

        // Keep one GPU panel per scene panel, recreating only those whose grid
        // changed (so a takeover starting doesn't rebuild the ticker).
        self.panels.truncate(scene.panels.len());
        for (i, p) in scene.panels.iter_mut().enumerate() {
            let dims = (p.grid.cols, p.grid.rows);
            if i >= self.panels.len() {
                self.panels.push(PanelGpu::new(device, dims.0, dims.1));
                p.band.uploads.insert(0, Upload::Full);
            } else if self.panels[i].dims() != dims {
                self.panels[i] = PanelGpu::new(device, dims.0, dims.1);
                p.band.uploads.insert(0, Upload::Full);
            }
        }
        let overlay_opacity = scene.takeover_opacity();
        if scene.dimmed && self.shade.is_none() {
            let shade = self.ui_pipeline.layer(device, 1, 1);
            shade.upload(queue, &[0, 0, 0, 255]);
            self.shade = Some(shade);
        }

        // UI canvases: one texture per layer, re-uploaded only when redrawn.
        self.ui.truncate(scene.ui.len());
        // Scrolling layers (the crawl) are packed into tiles to fit.
        for (i, layer) in scene.ui.iter_mut().enumerate() {
            let c = &layer.canvas;
            let packed = layer.scroll.is_some().then(|| pack_tiles(&c.data, c.width, c.height, STRIP_TILE_W));
            let size = packed.as_ref().map_or((c.width, c.height), |(size, _)| *size);
            let size = (size.0.max(1), size.1.max(1));
            if i >= self.ui.len() {
                self.ui.push(self.ui_pipeline.layer(device, size.0, size.1));
                layer.dirty = true;
            } else if self.ui[i].size() != size {
                self.ui[i] = self.ui_pipeline.layer(device, size.0, size.1);
                layer.dirty = true;
            }
            if layer.dirty {
                self.ui[i].upload(queue, packed.as_ref().map_or(&layer.canvas.data, |(_, data)| data));
                layer.dirty = false;
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
            let srgb_flag = f32::from(u8::from(self.pipes.manual_srgb));
            let (dot, glow, flicker, mode) = if panel.overlay {
                (BLOCK_SIZE, BLOCK_GLOW, 0.0, [1.0, 1.0, overlay_opacity, 0.0])
            } else {
                (cfg.dot_size, cfg.glow, cfg.flicker, [0.0, 0.0, 1.0, 0.0])
            };
            let params = Params {
                origin_pitch: [g.origin.0 as f32, g.origin.1 as f32, g.pitch as f32, dot],
                grid: [g.cols as f32, g.rows as f32, panel.band.strip.width() as f32, STRIP_TILE_W as f32],
                scroll: [base as f32, frac, f32::from(u8::from(smooth)), f32::from(u8::from(panel.band.wrap))],
                look: [glow, flicker, (scene.time % 3600.0) as f32, 1.0],
                off_color: linear(unlit, 0.0),
                bg: linear(BAND_BG, srgb_flag),
                blur: [0.0; 4],
                mode,
            };
            if panel.visible {
                gpu.prepare(device, queue, &self.pipes, &mut encoder, &params);
            }
        }

        let takeover = scene.takeover_view();
        if let Some(view) = &takeover {
            let srgb_flag = f32::from(u8::from(self.pipes.manual_srgb));
            let boxes: Vec<([f32; 4], [f32; 4])> = view
                .boxes
                .iter()
                .map(|(r, radius, c)| {
                    (rect4(*r), {
                        let mut l = linear(*c, 0.0);
                        l[3] = *radius;
                        l
                    })
                })
                .collect();
            let none = ([0.0; 4], [0.0; 4]);
            let (b0, b1) = (boxes.first().copied().unwrap_or(none), boxes.get(1).copied().unwrap_or(none));
            self.takeover.write(
                queue,
                &TakeoverParams {
                    rect: rect4(view.area),
                    stripe_a: linear(view.palette.stripe_a, view.opacity),
                    stripe_b: linear(view.palette.stripe_b, (scene.time % 3600.0) as f32),
                    bar: linear(view.palette.bar, srgb_flag),
                    box0: b0.0,
                    box0_color: b0.1,
                    box1: b1.0,
                    box1_color: b1.1,
                    pattern: {
                        let p = view.palette.pattern;
                        [p.dir.0, p.dir.1, p.width, p.speed]
                    },
                    dots: [view.palette.pattern.dots, view.palette.pattern.cell, 0.0, 0.0],
                },
            );
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
            // Base panels, then the takeover background, then its LED text.
            for (gpu, panel) in self.panels.iter_mut().zip(&scene.panels).filter(|(_, p)| p.visible && !p.overlay) {
                let b = panel.grid.band;
                gpu.composite(device, &self.pipes, &mut pass, [b.x, b.y, b.w, b.h], false);
            }
            for (gpu, layer) in self.ui.iter().zip(&scene.ui).filter(|(_, l)| l.opacity > 0.0) {
                let r = layer.rect;
                let scroll =
                    layer.scroll.map(|s| (s.offset(layer.canvas.width), layer.canvas.width, layer.canvas.height));
                gpu.draw(
                    queue,
                    &self.ui_pipeline,
                    &mut pass,
                    [r.x, r.y, r.w, r.h],
                    layer.opacity,
                    self.pipes.manual_srgb,
                    scroll,
                );
            }
            if let Some(view) = &takeover {
                let a = view.area;
                self.takeover.draw(&mut pass, [a.x, a.y, a.w, a.h]);
            }
            for (gpu, panel) in self.panels.iter_mut().zip(&scene.panels).filter(|(_, p)| p.visible && p.overlay) {
                let b = panel.grid.band;
                gpu.composite(device, &self.pipes, &mut pass, [b.x, b.y, b.w, b.h], true);
            }
            if let Some(shade) = self.shade.as_ref().filter(|_| scene.dimmed) {
                let (w, h) = (scene.layout.width, scene.layout.height);
                shade.draw(queue, &self.ui_pipeline, &mut pass, [0, 0, w, h], DIM, self.pipes.manual_srgb, None);
            }
        }
        queue.submit([encoder.finish()]);
    }
}
