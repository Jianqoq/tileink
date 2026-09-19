// Resize recording into one persistent allocation, including GPU completion.
use criterion::{Criterion, criterion_group, criterion_main};
use tileink::{
    Canvas, ExternalTextureHistoryId, NativeBackend, NativeContext, NativeContextOptions,
    NativeRenderTarget, NativeRenderer, Radius,
};

fn resize_with_capacity(c: &mut Criterion) {
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU").expect("pin the GPU")),
            validation: false,
        },
    )
    .unwrap();
    let texture = context.create_texture(384, 288).unwrap();
    let target = NativeRenderTarget::persistent(&texture, ExternalTextureHistoryId::new(1));
    let scenes = [(257, 193), (241, 179), (255, 191)].map(|(width, height)| {
        let mut canvas = Canvas::new(width, height, 1.0);
        canvas.push_rect(
            peniko::kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
            Radius::ZERO,
            peniko::Color::from_rgb8(17, 31, 53),
        );
        canvas
    });
    let mut renderer = NativeRenderer::with_context(&context, 257, 193).unwrap();
    for scene in &scenes {
        renderer.render_to_target(scene, target).unwrap().wait().unwrap();
    }
    c.bench_function("native_resize_with_target_capacity", |b| {
        b.iter(|| {
            for scene in &scenes {
                renderer.render_to_target(scene, target).unwrap().wait().unwrap();
            }
        });
    });
}

criterion_group!(benches, resize_with_capacity);
criterion_main!(benches);
