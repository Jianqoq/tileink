use std::time::Duration;

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{CubeWgpuRenderer, Radius, Scene, WgpuRenderer};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const PATH_COUNT: usize = 256;

struct BenchContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cubecl: CubeWgpuRenderer,
    native: WgpuRenderer,
    cubecl_target: wgpu::Texture,
    native_target: wgpu::Texture,
}

impl BenchContext {
    fn new() -> Self {
        let setup = ::cubecl::wgpu::init_setup::<::cubecl::wgpu::AutoGraphicsApi>(
            &::cubecl::wgpu::WgpuDevice::DefaultDevice,
            ::cubecl::wgpu::RuntimeOptions::default(),
        );
        let device = setup.device.clone();
        let queue = setup.queue.clone();
        assert!(
            device
                .features()
                .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES),
            "native wgpu texture target path requires TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES"
        );

        let cube_device =
            ::cubecl::wgpu::init_device(setup, ::cubecl::wgpu::RuntimeOptions::default());
        let cubecl = CubeWgpuRenderer::new(&cube_device, WIDTH, HEIGHT, Color::WHITE);
        let native = WgpuRenderer::new(&device, &queue, WIDTH, HEIGHT, Color::WHITE);
        let cubecl_target = create_target(&device, "tileink cubecl compare target");
        let native_target = create_target(&device, "tileink native wgpu compare target");
        Self {
            device,
            queue,
            cubecl,
            native,
            cubecl_target,
            native_target,
        }
    }

    fn sync(&self) {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll wgpu device");
    }
}

fn create_target(device: &wgpu::Device, label: &'static str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn build_scene() -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.push_rect(
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        Radius::ZERO,
        Color::WHITE,
    );
    for i in 0..PATH_COUNT {
        let x = WIDTH as f64 * 0.5 + (i % 16) as f64 * 7.0 - 52.5;
        let y = HEIGHT as f64 * 0.5 + (i / 16) as f64 * 5.0 - 37.5;
        let radius = 390.0 - (i % 23) as f64 * 3.0;
        scene.push_circle(Circle::new((x, y), radius), color_at(i));
    }
    scene
}

fn color_at(i: usize) -> Color {
    Color::from_rgba8(
        40 + (i * 17 % 150) as u8,
        70 + (i * 11 % 150) as u8,
        210,
        170,
    )
}

fn bench_wgpu_cubecl_compare(c: &mut Criterion) {
    let scene = build_scene();
    let mut context = BenchContext::new();

    context
        .cubecl
        .render_to_wgpu_texture(
            &scene,
            &context.device,
            &context.queue,
            &context.cubecl_target,
        )
        .expect("warm cubecl render to texture");
    context.sync();
    context
        .native
        .render_to_wgpu_texture(&scene, &context.native_target)
        .expect("warm native wgpu render to texture");
    context.sync();

    let mut group = c.benchmark_group("wgpu_cubecl_compare_1080p");
    group.throughput(Throughput::Elements((WIDTH * HEIGHT) as u64));

    group.bench_function("cubecl_render_to_wgpu_texture", |b| {
        b.iter(|| {
            context
                .cubecl
                .render_to_wgpu_texture(
                    black_box(&scene),
                    black_box(&context.device),
                    black_box(&context.queue),
                    black_box(&context.cubecl_target),
                )
                .expect("cubecl render to texture");
            context.sync();
        });
    });

    group.bench_function("native_wgpu_render_to_wgpu_texture", |b| {
        b.iter(|| {
            context
                .native
                .render_to_wgpu_texture(black_box(&scene), black_box(&context.native_target))
                .expect("native wgpu render to texture");
            context.sync();
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = bench_wgpu_cubecl_compare
}
criterion_main!(benches);
