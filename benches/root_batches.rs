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

fn root_batches(width: u32, height: u32, batches: u32) -> Canvas {
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
            rect,
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
        for (width, height, batches) in [
            (256, 256, 32),
            (1600, 1000, 4),
            (1600, 1000, 18),
            (1600, 1000, 32),
            // Resize also exercises partial tiles and partial prefix chunks.
            (1601, 1001, 32),
            (2560, 1440, 64),
        ] {
            let canvas = root_batches(width, height, batches);
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
            assert_eq!(stats.root_draw_batches, batches + 1);
            assert_eq!(stats.portable_texture_copies, if portable { 2 } else { 0 });
            eprintln!(
                "{mode} {width}x{height}/{batches}: root_batches={}, submissions={}, portable_copies={}",
                stats.root_draw_batches, stats.queue_submissions, stats.portable_texture_copies
            );
            group.bench_function(format!("{width}x{height}/{batches}"), |b| {
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
