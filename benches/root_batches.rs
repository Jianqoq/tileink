use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius, Renderer};

fn benchmark_device(portable: bool) -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("root-batch benchmark requires a GPU adapter");
    eprintln!("root-batch adapter: {:?}", adapter.get_info());
    let native = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
        | wgpu::Features::TEXTURE_BINDING_ARRAY
        | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink root-batch benchmark device"),
        required_features: if portable {
            wgpu::Features::empty()
        } else {
            native
        },
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("root-batch benchmark requires the requested texture path")
}

fn root_batches(width: u32, height: u32, batches: u32, sparse: bool) -> Canvas {
    let mut canvas = Canvas::new(width, height, 1.0);
    canvas.push_rect(
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        Radius::ZERO,
        Color::from_rgb8(12, 18, 28),
    );
    for index in 0..batches {
        let inset = f64::from(index) * f64::from(width.min(height)) / f64::from(batches * 4);
        let rect = Rect::new(
            inset,
            inset,
            f64::from(width) - inset,
            f64::from(height) - inset,
        );
        canvas.push_clip_sdf_rect_layer(rect, Radius::all(8.0));
        canvas.push_rect(
            if sparse {
                Rect::new(inset + 8.0, inset + 8.0, inset + 40.0, inset + 40.0)
            } else {
                rect
            },
            Radius::ZERO,
            if index % 2 == 0 {
                Color::from_rgba8(28, 108, 228, 18)
            } else {
                Color::from_rgba8(74, 198, 148, 18)
            },
        );
        canvas.pop_layer();
    }
    canvas
}

fn benchmark_scenes() -> Vec<(String, Canvas, u32)> {
    let mut scenes = Vec::new();
    for (width, height, batches) in [1, 2, 4, 8, 16, 32, 64]
        .map(|batches| (1600, 1000, batches))
        .into_iter()
        .chain([
            (800, 500, 32),
            (1601, 1001, 32),
            (2560, 1440, 32),
            (3840, 2160, 32),
        ])
    {
        for sparse in [false, true] {
            if sparse && batches == 1 {
                continue;
            }
            let kind = if sparse { "sparse" } else { "dense" };
            scenes.push((
                format!("{kind}-{width}x{height}-b{batches}"),
                root_batches(width, height, batches - 1, sparse),
                batches,
            ));
        }
    }
    let tiger = usvg::Tree::from_data(
        include_bytes!("../examples/tiger.svg"),
        &usvg::Options::default(),
    )
    .unwrap();
    for (size, batches) in [400, 800, 1600, 2048, 2304, 3200]
        .map(|size| (size, 1))
        .into_iter()
        .chain(
            [800, 1600, 3200]
                .into_iter()
                .flat_map(|size| [2, 8].map(|batches| (size, batches))),
        )
    {
        let mut canvas = Canvas::new(size, size, 1.0);
        for layer in 0..batches {
            if batches > 1 {
                let inset = f64::from(layer) * 0.5;
                canvas.push_clip_sdf_rect_layer(
                    Rect::new(
                        inset,
                        inset,
                        f64::from(size) - inset,
                        f64::from(size) - inset,
                    ),
                    Radius::all(4.0),
                );
            }
            canvas
                .push_svg_with_options(
                    &tiger,
                    tileink::SvgOptions {
                        transform: peniko::kurbo::Affine::scale_non_uniform(
                            f64::from(size) / f64::from(tiger.size().width()),
                            f64::from(size) / f64::from(tiger.size().height()),
                        ),
                        ..tileink::SvgOptions::default()
                    },
                )
                .unwrap();
            if batches > 1 {
                canvas.pop_layer();
            }
        }
        scenes.push((format!("tiger-{size}-b{batches}"), canvas, batches));
    }
    scenes
}

fn root_batch_submission(c: &mut Criterion) {
    // Keep both texture paths and small workloads in the matrix: an extra submit only helps
    // when enough GPU work can overlap the remaining CPU command-buffer completion.
    for portable in [false, true] {
        let (device, queue) = benchmark_device(portable);
        let mode = if portable { "portable" } else { "native" };
        let mut group = c.benchmark_group(format!("{mode}_root_batches"));
        group.sample_size(20);
        group.warm_up_time(Duration::from_secs(2));
        group.measurement_time(Duration::from_secs(4));
        let mut renderer = Renderer::new(&device, &queue, 1600, 1000, Color::TRANSPARENT);
        for (name, canvas, batches) in benchmark_scenes() {
            let width = canvas.physical_width();
            let height = canvas.physical_height();
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tileink root-batch benchmark target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            renderer
                .render_to_wgpu_texture(&canvas, &target)
                .expect("benchmark warmup must render");
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("benchmark warmup must complete");
            let stats = renderer.incremental_render_stats();
            assert_eq!(stats.root_draw_batches, batches);
            assert_eq!(stats.portable_texture_copies, if portable { 2 } else { 0 });
            eprintln!(
                "{mode} {width}x{height}/{batches}: root_batches={}, submissions={}, portable_copies={}",
                stats.root_draw_batches, stats.queue_submissions, stats.portable_texture_copies
            );
            group.bench_function(name, |b| {
                b.iter_custom(|iterations| {
                    let start = Instant::now();
                    for _ in 0..iterations {
                        renderer
                            .render_to_wgpu_texture(&canvas, &target)
                            .expect("root-batch frame must render");
                        device
                            .poll(wgpu::PollType::wait_indefinitely())
                            .expect("root-batch frame must complete");
                    }
                    start.elapsed()
                });
            });
        }
        group.finish();
    }
}

criterion_group!(benches, root_batch_submission);
criterion_main!(benches);
