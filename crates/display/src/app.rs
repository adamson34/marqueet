//! The windowed / full-screen app loop.

use std::sync::Arc;
use std::time::Instant;

use chrono::{Local, Utc};
use tickadee_core::config::DisplayConfig;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::render::{self, Renderer};
use crate::scene::{Scene, SceneSetup};

pub fn run(config: DisplayConfig, size: (u32, u32), fullscreen: bool, seed: u64) -> render::Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    // GLES (e.g. on Wayland / Ubuntu Frame) needs the display handle up front.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
        event_loop.owned_display_handle(),
    )));
    let mut app = App { config, size, fullscreen, seed, instance, state: None, error: None };
    event_loop.run_app(&mut app)?;
    match app.error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

struct App {
    config: DisplayConfig,
    size: (u32, u32),
    fullscreen: bool,
    seed: u64,
    instance: wgpu::Instance,
    state: Option<State>,
    error: Option<Box<dyn std::error::Error + Send + Sync>>,
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    scene: Scene,
    last_frame: Instant,
    stats: FrameStats,
}

/// Logs frame rate and CPU time per frame every few seconds, to spot
/// hardware that can't keep up (e.g. when testing on a Raspberry Pi).
struct FrameStats {
    since: Instant,
    frames: u32,
    busy: std::time::Duration,
}

impl FrameStats {
    const INTERVAL_SECS: f64 = 10.0;

    fn record(&mut self, busy: std::time::Duration) {
        self.frames += 1;
        self.busy += busy;
        let elapsed = self.since.elapsed().as_secs_f64();
        if elapsed >= Self::INTERVAL_SECS {
            let fps = f64::from(self.frames) / elapsed;
            let cpu_ms = self.busy.as_secs_f64() * 1000.0 / f64::from(self.frames.max(1));
            log::info!("{fps:.1} fps, {cpu_ms:.2} ms CPU per frame");
            *self = FrameStats { since: Instant::now(), frames: 0, busy: Default::default() };
        }
    }
}

impl App {
    fn init(&mut self, event_loop: &ActiveEventLoop) -> render::Result<State> {
        let mut attrs = Window::default_attributes()
            .with_title("Tickadee")
            .with_inner_size(PhysicalSize::new(self.size.0, self.size.1));
        if self.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = Arc::new(event_loop.create_window(attrs)?);
        if self.fullscreen {
            window.set_cursor_visible(false);
        }
        let surface = self.instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))?;
        let (device, queue) = pollster::block_on(render::request_device(&adapter))?;

        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface not supported by this GPU")?;
        let caps = surface.get_capabilities(&adapter);
        if let Some(srgb) = caps.formats.iter().copied().find(wgpu::TextureFormat::is_srgb) {
            surface_config.format = srgb;
        }
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &surface_config);

        let renderer = Renderer::new(&device, surface_config.format);
        let scene = Scene::new(
            self.config.clone(),
            surface_config.width,
            surface_config.height,
            SceneSetup {
                now: Utc::now(),
                tz: *Local::now().offset(),
                seed: self.seed,
                max_strip_width: Renderer::max_strip_width(self.config.ticker_rows.max(self.config.crawl_rows)),
            },
        );
        let stats = FrameStats { since: Instant::now(), frames: 0, busy: Default::default() };
        Ok(State { window, surface, surface_config, device, queue, renderer, scene, last_frame: Instant::now(), stats })
    }
}

impl State {
    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.surface_config.width = w;
        self.surface_config.height = h;
        self.surface.configure(&self.device, &self.surface_config);
        self.scene.resize(w, h, Utc::now());
    }

    fn frame(&mut self) {
        let now = Instant::now();
        // Clamp so a stall (e.g. window drag) doesn't teleport the ticker.
        let dt = now.duration_since(self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        self.scene.update(dt, Utc::now());

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => return,
            wgpu::CurrentSurfaceTexture::Validation => {
                log::warn!("surface validation error; skipping frame");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        self.renderer.render(&self.device, &self.queue, &mut self.scene, &view);
        self.stats.record(now.elapsed());
        self.window.pre_present_notify();
        self.queue.present(frame);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match self.init(event_loop) {
            Ok(state) => {
                state.window.request_redraw();
                self.state = Some(state);
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, .. }, ..
            } => match logical_key.as_ref() {
                Key::Named(NamedKey::Escape) | Key::Character("q") => event_loop.exit(),
                Key::Character("f") | Key::Named(NamedKey::F11) => {
                    let full = state.window.fullscreen().is_some();
                    state.window.set_fullscreen((!full).then_some(Fullscreen::Borderless(None)));
                    state.window.set_cursor_visible(full);
                }
                _ => {}
            },
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                state.frame();
                state.window.request_redraw();
            }
            _ => {}
        }
    }
}
