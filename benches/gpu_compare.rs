mod common;

use std::num::NonZeroUsize;

use common::{
    HEIGHT, WIDTH, build_tileink_scene, circle_at, color_at, prepared_cubecl_renderer, sync_cubecl,
};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use tileink::CubeWgpuRenderer;

struct VelloWgpuContext {
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    _target: wgpu::Texture,
    target_view: wgpu::TextureView,
}

impl VelloWgpuContext {
    fn new() -> Self {
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
        let (device, queue) =
            cubecl_common::future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("tileink_vello_compare_device"),
                required_features: adapter.features() & maybe_features,
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            }))
            .expect("create wgpu device for Vello benchmark");
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tileink_vello_compare_target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
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

    fn sync(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }
}

fn build_vello_scene(path_count: usize, dense: bool) -> vello::Scene {
    let mut scene = vello::Scene::new();
    for i in 0..path_count {
        scene.fill(
            vello::peniko::Fill::NonZero,
            vello::kurbo::Affine::IDENTITY,
            color_at(i),
            None,
            &circle_at(i, dense),
        );
    }
    scene
}

fn run_cubecl_prepared(renderer: &mut CubeWgpuRenderer) {
    renderer.scan();
    renderer.cumsum();
    renderer.coarse();
    renderer.fine();
}

fn benchmark_case(c: &mut Criterion, path_count: usize, dense: bool) {
    let tileink_scene = build_tileink_scene(path_count, dense);
    let mut cubecl_renderer = prepared_cubecl_renderer(&tileink_scene);
    run_cubecl_prepared(&mut cubecl_renderer);
    sync_cubecl(&cubecl_renderer);

    let vello_context = VelloWgpuContext::new();
    let vello_scene = build_vello_scene(path_count, dense);
    let mut vello_renderer = vello::Renderer::new(
        &vello_context.device,
        vello::RendererOptions {
            use_cpu: false,
            num_init_threads: NonZeroUsize::new(1),
            antialiasing_support: vello::AaSupport::area_only(),
            ..Default::default()
        },
    )
    .expect("create Vello renderer");
    let vello_params = vello::RenderParams {
        base_color: vello::peniko::Color::WHITE,
        width: WIDTH,
        height: HEIGHT,
        antialiasing_method: vello::AaConfig::Area,
    };
    vello_renderer
        .render_to_texture(
            &vello_context.device,
            &vello_context.queue,
            &vello_scene,
            &vello_context.target_view,
            &vello_params,
        )
        .expect("Vello warmup");
    vello_context.sync();

    let scenario = if dense { "dense" } else { "distributed" };
    let mut group = c.benchmark_group(format!("gpu_compare/{scenario}"));
    group.throughput(Throughput::Elements((WIDTH * HEIGHT) as u64));

    group.bench_with_input(
        BenchmarkId::new("cubecl_gpu_prepared", path_count),
        &path_count,
        |b, _| {
            b.iter(|| {
                run_cubecl_prepared(black_box(&mut cubecl_renderer));
                sync_cubecl(&cubecl_renderer);
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::new("cubecl_gpu_with_prepare", path_count),
        &path_count,
        |b, _| {
            let mut renderer =
                CubeWgpuRenderer::new_default_device(WIDTH, HEIGHT, vello::peniko::Color::WHITE);
            b.iter(|| {
                renderer.render(black_box(&tileink_scene));
                sync_cubecl(&renderer);
            });
        },
    );

    group.bench_with_input(
        BenchmarkId::new("vello_gpu_area", path_count),
        &path_count,
        |b, _| {
            b.iter(|| {
                vello_renderer
                    .render_to_texture(
                        black_box(&vello_context.device),
                        black_box(&vello_context.queue),
                        black_box(&vello_scene),
                        black_box(&vello_context.target_view),
                        black_box(&vello_params),
                    )
                    .expect("Vello render");
                vello_context.sync();
            });
        },
    );

    group.finish();
}

fn gpu_compare(c: &mut Criterion) {
    benchmark_case(c, 64, false);
    benchmark_case(c, 256, false);
    benchmark_case(c, 64, true);
    benchmark_case(c, 128, true);
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = gpu_compare
}
criterion_main!(benches);
