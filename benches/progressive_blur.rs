use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{
    Canvas, Filter, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer,
    ProgressiveBlur, Radius, Region,
};

fn progressive_blur(c: &mut Criterion) {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(
                std::env::var("TILEINK_BENCH_GPU").expect("pin the physical GPU identity"),
            ),
            validation: false,
        },
    )
    .unwrap();
    let mut group = c.benchmark_group("progressive_blur_submit_and_wait");
    group.sample_size(30);
    for (width, height, sigma) in [
        (512, 256, 8.0),
        (512, 256, 32.0),
        (1920, 1080, 32.0),
        (1920, 1080, 128.0),
    ] {
        let rect = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
        let mut canvas = Canvas::new(width, height, 1.0);
        canvas
            .push_checkerboard(rect, 8.0, Color::BLACK, Color::WHITE)
            .unwrap();
        canvas.push_backdrop_layer(
            Filter::ProgressiveBlur(ProgressiveBlur::new(
                Point::new(0.0, f64::from(height) * 0.1),
                Point::new(0.0, f64::from(height) * 0.9),
                sigma,
            )),
            Region::rect(rect, Radius::ZERO),
        );
        canvas.pop_layer();
        let mut renderer = NativeRenderer::with_context(&context, width, height).unwrap();
        let target = context.create_texture(width, height).unwrap();
        // Warm pipeline creation, shader compilation and pooled allocations.
        for _ in 0..5 {
            renderer
                .render_to_texture(&canvas, &target)
                .unwrap()
                .wait()
                .unwrap();
        }
        group.bench_with_input(
            BenchmarkId::new(format!("{width}x{height}"), sigma),
            &canvas,
            |b, canvas| {
                b.iter(|| {
                    renderer
                        .render_to_texture(canvas, &target)
                        .unwrap()
                        .wait()
                        .unwrap()
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, progressive_blur);
criterion_main!(benches);
