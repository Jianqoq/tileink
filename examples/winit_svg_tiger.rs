// Kept outside examples/cpu and examples/wgpu so scripts/run_examples.ps1 only
// runs the headless image-producing examples.
#[path = "common/mod.rs"]
mod common;

use std::{error::Error, sync::Arc};

use peniko::Color;
use tileink::{Canvas, Renderer};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalSize},
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

const TARGET_WIDTH: u32 = 900;

fn main() -> Result<(), Box<dyn Error>> {
    let mut app = App::new()?;
    EventLoop::new()?.run_app(&mut app)?;
    Ok(())
}

struct App {
    scene: Canvas,
    scene_width: u32,
    scene_height: u32,
    state: Option<State>,
}

impl App {
    fn new() -> Result<Self, Box<dyn Error>> {
        let input = common::example_asset("tiger.svg");
        let (scene, scene_width, scene_height) = common::load_svg_scene(input, TARGET_WIDTH)?;
        Ok(Self {
            scene,
            scene_width,
            scene_height,
            state: None,
        })
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Tileink SVG tiger (winit)")
            .with_resizable(false)
            .with_inner_size(LogicalSize::new(
                self.scene_width as f64,
                self.scene_height as f64,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        match State::new(window, self.scene_width, self.scene_height) {
            Ok(state) => {
                state.window.request_redraw();
                self.state = Some(state);
            }
            Err(err) => {
                eprintln!("failed to initialize wgpu example: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.state else {
            return;
        };
        if window_id != state.window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size);
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = state.draw(&self.scene) {
                    eprintln!("failed to draw frame: {err}");
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    renderer: Renderer,
}

impl State {
    fn new(
        window: Arc<Window>,
        scene_width: u32,
        scene_height: u32,
    ) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))?;
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("tileink winit tiger device"),
                required_features: adapter
                    .features()
                    .difference(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS),
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            }),
        )?;

        let window_size = window.inner_size();
        let config = surface_config(&surface, &adapter, window_size)?;
        surface.configure(&device, &config);

        let renderer = Renderer::new(&device, &queue, scene_width, scene_height, Color::WHITE);
        Ok(Self {
            window,
            surface,
            config,
            device,
            renderer,
        })
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    fn draw(&mut self, scene: &Canvas) -> Result<(), Box<dyn Error>> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(std::io::Error::other("surface validation error").into());
            }
        };

        self.renderer
            .render_to_wgpu_texture(scene, &frame.texture)?;
        self.renderer.queue().present(frame);
        Ok(())
    }
}

fn surface_config(
    surface: &wgpu::Surface<'_>,
    adapter: &wgpu::Adapter,
    size: PhysicalSize<u32>,
) -> Result<wgpu::SurfaceConfiguration, Box<dyn Error>> {
    let width = size.width.max(1);
    let height = size.height.max(1);
    let capabilities = surface.get_capabilities(adapter);
    let format = [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ]
    .into_iter()
    .find(|format| capabilities.formats.contains(format))
    .ok_or_else(|| {
        std::io::Error::other(format!(
            "surface does not support direct RGBA8 blit; formats: {:?}",
            capabilities.formats
        ))
    })?;
    let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::STORAGE_BINDING;
    if !capabilities.usages.contains(usage) {
        return Err(std::io::Error::other(format!(
            "surface does not support STORAGE_BINDING usage; usages: {:?}",
            capabilities.usages
        ))
        .into());
    }
    Ok(wgpu::SurfaceConfiguration {
        usage,
        format,
        color_space: wgpu::SurfaceColorSpace::Auto,
        width,
        height,
        present_mode: capabilities
            .present_modes
            .first()
            .copied()
            .ok_or_else(|| std::io::Error::other("surface has no present modes"))?,
        alpha_mode: capabilities
            .alpha_modes
            .first()
            .copied()
            .ok_or_else(|| std::io::Error::other("surface has no alpha modes"))?,
        desired_maximum_frame_latency: 2,
        view_formats: vec![],
    })
}
