//! wgpu resources for LED panels.

use bytemuck::{Pod, Zeroable};
use marqueet_core::ticker::LedBitmap;

/// Width of one tile of the strip texture. The strip is a very long, short
/// image; it is folded into rows of tiles so it fits texture size limits
/// (2048 is the WebGL2 / GLES 3.0 floor).
pub const STRIP_TILE_W: u32 = 2048;
pub const MAX_TEX: u32 = 2048;
const GRID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Params {
    pub origin_pitch: [f32; 4],
    pub grid: [f32; 4],
    pub scroll: [f32; 4],
    pub look: [f32; 4],
    pub off_color: [f32; 4],
    pub bg: [f32; 4],
    pub blur: [f32; 4],
    /// overlay (0/1), square dots (0/1), opacity, unused.
    pub mode: [f32; 4],
}

/// Pipelines shared by all panels.
#[derive(Debug)]
pub struct LedPipelines {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    gather: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// Composite with premultiplied-alpha blending, for LED text over a takeover.
    composite_overlay: wgpu::RenderPipeline,
    /// True when the output target is not sRGB and the shader must encode.
    pub manual_srgb: bool,
}

impl LedPipelines {
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("led.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("led.wgsl").into()),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("led"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex(1),
                tex(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("led"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, format, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("led"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        LedPipelines {
            gather: pipeline("fs_gather", GRID_FORMAT, None),
            blur: pipeline("fs_blur", GRID_FORMAT, None),
            composite: pipeline("fs_composite", output_format, None),
            composite_overlay: pipeline(
                "fs_composite",
                output_format,
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            ),
            layout,
            sampler,
            manual_srgb: !output_format.is_srgb(),
        }
    }
}

/// Splits columns `[start, start + width)` of a strip into per-tile pieces:
/// `(tile index, x within tile, offset into the source, width)`.
pub fn tile_pieces(start: u32, width: u32, tile_w: u32) -> Vec<(u32, u32, u32, u32)> {
    let mut out = Vec::new();
    let mut col = start;
    let end = start + width;
    while col < end {
        let tile = col / tile_w;
        let x = col % tile_w;
        let w = (tile_w - x).min(end - col);
        out.push((tile, x, col - start, w));
        col += w;
    }
    out
}

/// Number of tiles needed for a strip of `width` columns.
/// Longest scrolling UI strip `h` px tall that fits in one texture.
pub fn ui_strip_max(h: u32) -> u32 {
    (MAX_TEX / h.max(1)).max(1) * STRIP_TILE_W
}

/// Rearranges a `width` x `h` RGBA strip into tiles `tile_w` wide stacked
/// top to bottom, so a long strip fits the GPU's texture size limit. Returns
/// the texture size and pixels. Inverse of the lookup in `ui.wgsl`.
pub fn pack_tiles(rgba: &[u8], width: u32, h: u32, tile_w: u32) -> ((u32, u32), Vec<u8>) {
    if width <= tile_w {
        return ((width, h), rgba.to_vec());
    }
    let tiles = tiles_for(width, tile_w);
    let mut out = vec![0u8; (tile_w * h * tiles * 4) as usize];
    for t in 0..tiles {
        let x0 = t * tile_w;
        let n = (width - x0).min(tile_w) as usize * 4;
        for y in 0..h {
            let src = ((y * width + x0) * 4) as usize;
            let dst = (((t * h + y) * tile_w) * 4) as usize;
            out[dst..dst + n].copy_from_slice(&rgba[src..src + n]);
        }
    }
    ((tile_w, h * tiles), out)
}

pub fn tiles_for(width: u32, tile_w: u32) -> u32 {
    width.div_ceil(tile_w).max(1)
}

struct Target {
    view: wgpu::TextureView,
}

fn render_target(device: &wgpu::Device, w: u32, h: u32, label: &str) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: GRID_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    Target { view: texture.create_view(&Default::default()) }
}

/// GPU state for one LED panel (main ticker, crawl, or a static panel).
pub struct PanelGpu {
    rows: u32,
    cols: u32,
    tile_capacity: u32,
    strip: wgpu::Texture,
    strip_view: wgpu::TextureView,
    grid: Target,
    blur_a: Target,
    blur_b: Target,
    params: wgpu::Buffer,
    blur_h: wgpu::Buffer,
    blur_v: wgpu::Buffer,
    groups: Option<[wgpu::BindGroup; 4]>,
}

impl std::fmt::Debug for PanelGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelGpu").field("rows", &self.rows).field("cols", &self.cols).finish()
    }
}

impl PanelGpu {
    pub fn new(device: &wgpu::Device, cols: u32, rows: u32) -> Self {
        let uniform = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (strip, strip_view) = Self::create_strip(device, rows, 1);
        let (gw, gh) = (cols + 2, rows + 2);
        PanelGpu {
            rows,
            cols,
            tile_capacity: 1,
            strip,
            strip_view,
            grid: render_target(device, gw, gh, "led grid"),
            blur_a: render_target(device, gw, gh, "led blur a"),
            blur_b: render_target(device, gw, gh, "led blur b"),
            params: uniform("led params"),
            blur_h: uniform("led blur h"),
            blur_v: uniform("led blur v"),
            groups: None,
        }
    }

    fn create_strip(device: &wgpu::Device, rows: u32, tiles: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let strip = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("led strip"),
            size: wgpu::Extent3d { width: STRIP_TILE_W, height: rows * tiles, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GRID_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = strip.create_view(&Default::default());
        (strip, view)
    }

    /// Visible LED grid size this panel was created for.
    pub fn dims(&self) -> (u32, u32) {
        (self.cols, self.rows)
    }

    /// Most tiles a strip can have for this panel.
    fn max_tiles(&self) -> u32 {
        (MAX_TEX / self.rows).max(1)
    }

    /// Uploads a whole strip, growing the texture if needed.
    pub fn upload_strip(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bmp: &LedBitmap) {
        debug_assert_eq!(bmp.height, self.rows);
        let needed = tiles_for(bmp.width, STRIP_TILE_W).min(self.max_tiles());
        if needed > self.tile_capacity {
            let cap = needed.next_power_of_two().min(self.max_tiles());
            let (strip, view) = Self::create_strip(device, self.rows, cap);
            self.strip = strip;
            self.strip_view = view;
            self.tile_capacity = cap;
            self.groups = None;
        }
        self.upload_columns(queue, bmp, 0);
    }

    /// Uploads `bmp` (a slice of the strip) at strip column `start`.
    pub fn upload_columns(&self, queue: &wgpu::Queue, bmp: &LedBitmap, start: u32) {
        let limit = self.tile_capacity * STRIP_TILE_W;
        let width = bmp.width.min(limit.saturating_sub(start));
        for (tile, x, src, w) in tile_pieces(start, width, STRIP_TILE_W) {
            let mut data = Vec::with_capacity((w * self.rows * 4) as usize);
            for y in 0..self.rows {
                let row = ((y * bmp.width + src) * 4) as usize;
                data.extend_from_slice(&bmp.data[row..row + (w * 4) as usize]);
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.strip,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y: tile * self.rows, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &data,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(self.rows) },
                wgpu::Extent3d { width: w, height: self.rows, depth_or_array_layers: 1 },
            );
        }
    }

    fn bind_groups(&mut self, device: &wgpu::Device, pipes: &LedPipelines) -> &[wgpu::BindGroup; 4] {
        self.groups.get_or_insert_with(|| {
            let group = |label, buf: &wgpu::Buffer, a: &wgpu::TextureView, b: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &pipes.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(a) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(b) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&pipes.sampler) },
                    ],
                })
            };
            [
                group("gather", &self.params, &self.strip_view, &self.strip_view),
                group("blur h", &self.blur_h, &self.grid.view, &self.grid.view),
                group("blur v", &self.blur_v, &self.blur_a.view, &self.blur_a.view),
                group("composite", &self.params, &self.grid.view, &self.blur_b.view),
            ]
        })
    }

    /// Writes uniforms and records the offscreen passes (gather + blur).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipes: &LedPipelines,
        encoder: &mut wgpu::CommandEncoder,
        params: &Params,
    ) {
        let texel = [1.0 / (self.cols + 2) as f32, 1.0 / (self.rows + 2) as f32];
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(params));
        let blur = |dir: [f32; 2]| Params { blur: [dir[0], dir[1], texel[0], texel[1]], ..*params };
        queue.write_buffer(&self.blur_h, 0, bytemuck::bytes_of(&blur([1.0, 0.0])));
        queue.write_buffer(&self.blur_v, 0, bytemuck::bytes_of(&blur([0.0, 1.0])));

        let [gather, blur_h, blur_v, _] = self.bind_groups(device, pipes).clone();
        for (target, pipeline, group) in [
            (&self.grid.view, &pipes.gather, &gather),
            (&self.blur_a.view, &pipes.blur, &blur_h),
            (&self.blur_b.view, &pipes.blur, &blur_v),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("led offscreen"),
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
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Draws the panel into `rect` (x, y, w, h in pixels) of the current pass;
    /// `overlay` blends it over what's already there (see `Params::mode`).
    pub fn composite(
        &mut self,
        device: &wgpu::Device,
        pipes: &LedPipelines,
        pass: &mut wgpu::RenderPass<'_>,
        rect: [u32; 4],
        overlay: bool,
    ) {
        let group = self.bind_groups(device, pipes)[3].clone();
        let [x, y, w, h] = rect;
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, w, h);
        pass.set_pipeline(if overlay { &pipes.composite_overlay } else { &pipes.composite });
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Uniforms for `takeover.wgsl` (ten vec4<f32>).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct TakeoverParams {
    pub rect: [f32; 4],
    pub stripe_a: [f32; 4],
    pub stripe_b: [f32; 4],
    pub bar: [f32; 4],
    pub box0: [f32; 4],
    pub box0_color: [f32; 4],
    pub box1: [f32; 4],
    pub box1_color: [f32; 4],
    /// Stripe direction (x, y), period (px at 1920 wide), drift (px/s).
    pub pattern: [f32; 4],
    /// Dot strength (> 0 lighter, < 0 darker), spacing (px at 1920 wide).
    pub dots: [f32; 4],
}

/// Pipeline and uniforms for the takeover background.
#[derive(Debug)]
pub struct TakeoverGpu {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
}

impl TakeoverGpu {
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("takeover.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("takeover.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("takeover"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("takeover"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("takeover"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_bg"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("takeover params"),
            size: std::mem::size_of::<TakeoverParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("takeover"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        TakeoverGpu { pipeline, uniforms, group }
    }

    pub fn write(&self, queue: &wgpu::Queue, params: &TakeoverParams) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(params));
    }

    /// Draws the background into `rect` of the current pass.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, rect: [u32; 4]) {
        let [x, y, w, h] = rect;
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, w, h);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// GPU side of one UI canvas layer: a texture the size of its rect.
pub struct UiGpu {
    size: (u32, u32),
    texture: wgpu::Texture,
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
}

impl std::fmt::Debug for UiGpu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiGpu").field("size", &self.size).finish()
    }
}

/// Pipeline shared by all UI layers.
#[derive(Debug)]
pub struct UiPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl UiPipeline {
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ui.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ui"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ui"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: output_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        UiPipeline { pipeline, layout }
    }

    pub fn layer(&self, device: &wgpu::Device, width: u32, height: u32) -> UiGpu {
        let (width, height) = (width.max(1), height.max(1));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ui canvas"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ui params"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let view = texture.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ui"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            ],
        });
        UiGpu { size: (width, height), texture, uniforms, group }
    }
}

impl UiGpu {
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Uploads premultiplied RGBA8 pixels covering the whole layer.
    pub fn upload(&self, queue: &wgpu::Queue, rgba: &[u8]) {
        let (w, h) = self.size;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }

    /// Draws the layer at `rect` with `opacity`. With `scroll` = (offset px,
    /// strip width px, strip height px), the texture holds a strip packed by
    /// [`pack_tiles`] and is shown from `offset`, wrapping around.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        pipes: &UiPipeline,
        pass: &mut wgpu::RenderPass<'_>,
        rect: [u32; 4],
        opacity: f32,
        manual_srgb: bool,
        scroll: Option<(f32, u32, u32)>,
    ) {
        let [x, y, w, h] = rect;
        let (offset, strip_w, strip_h) = scroll.unwrap_or((0.0, 0, 0));
        let params: [f32; 12] = [
            x as f32,
            y as f32,
            w as f32,
            h as f32,
            opacity,
            f32::from(u8::from(manual_srgb)),
            0.0,
            0.0,
            offset,
            strip_w as f32,
            STRIP_TILE_W as f32,
            strip_h as f32,
        ];
        queue.write_buffer(&self.uniforms, 0, bytemuck::cast_slice(&params));
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(x, y, w, h);
        pass.set_pipeline(&pipes.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_within_one_tile() {
        assert_eq!(tile_pieces(10, 20, 100), vec![(0, 10, 0, 20)]);
    }

    #[test]
    fn pieces_split_across_tiles() {
        assert_eq!(tile_pieces(90, 230, 100), vec![(0, 90, 0, 10), (1, 0, 10, 100), (2, 0, 110, 100), (3, 0, 210, 20)]);
    }

    #[test]
    fn empty_range_has_no_pieces() {
        assert!(tile_pieces(5, 0, 100).is_empty());
    }

    #[test]
    fn packed_tiles_hold_every_pixel() {
        // 5 px wide, 2 tall, tiles 2 wide: pixel value = x + 10 * y.
        let (w, h) = (5u32, 2u32);
        let strip: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| [(x + 10 * y) as u8; 4])).collect();
        let ((tw, th), packed) = pack_tiles(&strip, w, h, 2);
        assert_eq!((tw, th), (2, 6));
        for y in 0..h {
            for x in 0..w {
                // Same lookup as ui.wgsl.
                let tile = x / 2;
                let (px, py) = (x - tile * 2, y + tile * h);
                assert_eq!(packed[((py * tw + px) * 4) as usize], (x + 10 * y) as u8, "({x},{y})");
            }
        }
        let small: Vec<u8> = vec![7; 16];
        assert_eq!(pack_tiles(&small, 2, 2, 4), ((2, 2), small.clone()));
    }

    #[test]
    fn tile_count() {
        assert_eq!(tiles_for(0, 2048), 1);
        assert_eq!(tiles_for(2048, 2048), 1);
        assert_eq!(tiles_for(2049, 2048), 2);
    }

    #[test]
    fn takeover_params_layout_matches_shader() {
        assert_eq!(std::mem::size_of::<TakeoverParams>(), 10 * 16);
    }

    #[test]
    fn params_layout_matches_shader() {
        // Eight vec4<f32> in the WGSL struct.
        assert_eq!(std::mem::size_of::<Params>(), 8 * 16);
    }
}
