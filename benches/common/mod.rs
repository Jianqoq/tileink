// Each Criterion target compiles this shared module independently and uses a different helper set.
#![allow(dead_code)]

use std::num::NonZeroUsize;

use cubecl::prelude::Runtime;
use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{CubeRenderer, CubeWgpuRenderer, Scene};

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

pub fn color_at(i: usize) -> Color {
    Color::from_rgba8(
        40 + (i * 17 % 150) as u8,
        70 + (i * 11 % 150) as u8,
        210,
        170,
    )
}

pub fn circle_at(i: usize, dense: bool) -> Circle {
    if dense {
        Circle::new(
            (
                WIDTH as f64 * 0.5 + (i % 7) as f64 - 3.0,
                HEIGHT as f64 * 0.5 + (i % 5) as f64 - 2.0,
            ),
            245.0 - (i % 11) as f64,
        )
    } else {
        let x = i % 20;
        let y = i / 20;
        Circle::new((32.0 + x as f64 * 61.0, 32.0 + y as f64 * 52.0), 22.0)
    }
}

pub fn build_tileink_scene(path_count: usize, dense: bool) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.push_rect(
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        tileink::Radius::ZERO,
        Color::WHITE,
    );

    for i in 0..path_count {
        scene.push_circle(circle_at(i, dense), color_at(i));
    }

    scene
}

pub fn prepared_cubecl_renderer(scene: &Scene) -> CubeWgpuRenderer {
    let mut renderer = CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::WHITE);
    // Prepared stage benchmarks intentionally exclude scene upload and buffer preallocation.
    renderer.prepare_scene_for_bench(scene);
    sync_cubecl(&renderer);
    renderer
}

pub fn sync_cubecl<R: Runtime>(renderer: &CubeRenderer<R>) {
    // CubeCL launches asynchronously; sync measures GPU completion without target readback.
    cubecl_common::future::block_on(renderer.client_for_bench().sync())
        .expect("CubeCL sync failed");
}

pub struct VelloWgpuContext {
    _instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    _target: wgpu::Texture,
    pub target_view: wgpu::TextureView,
}

impl VelloWgpuContext {
    pub fn new(width: u32, height: u32, label: &str) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            display: None,
            backends: wgpu::Backends::from_env().unwrap_or_default(),
            flags: wgpu::InstanceFlags::from_build_config().with_env(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::from_env_or_default(),
        });
        let adapter = cubecl_common::future::block_on(
            wgpu::util::initialize_adapter_from_env_or_default(&instance, None),
        )
        .expect("create wgpu adapter for Vello benchmark");
        let maybe_features = wgpu::Features::CLEAR_TEXTURE | wgpu::Features::PIPELINE_CACHE;
        let device_label = format!("{label}_device");
        let (device, queue) =
            cubecl_common::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some(&device_label),
                required_features: adapter.features() & maybe_features,
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            }))
            .expect("create wgpu device for Vello benchmark");
        let target_label = format!("{label}_target");
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&target_label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            _instance: instance,
            device,
            queue,
            _target: target,
            target_view,
        }
    }

    pub fn sync(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

pub fn vello_renderer(device: &wgpu::Device) -> vello::Renderer {
    vello::Renderer::new(
        device,
        vello::RendererOptions {
            use_cpu: false,
            num_init_threads: NonZeroUsize::new(1),
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .expect("create Vello renderer")
}
