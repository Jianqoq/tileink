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

    // Isolated layers require full-canvas scratch leases. Alternating nearby
    // extents exposes allocation/residency churn hidden by a fixed-size scene.
    use peniko::kurbo::{Affine, Shape};
    let texture = context.create_texture(1760, 1030).unwrap();
    let target = NativeRenderTarget::persistent(&texture, ExternalTextureHistoryId::new(2));
    let scenes = [1600, 1608, 1592].map(|width| {
        let mut canvas = Canvas::new(width, 1030, 1.0);
        let bounds = peniko::kurbo::Rect::new(0.0, 0.0, width as f64, 1030.0);
        canvas.push_isolate_layer(bounds.to_path(0.1), Affine::IDENTITY, 0.1);
        canvas.push_rect(bounds, Radius::ZERO, peniko::Color::from_rgb8(17, 31, 53));
        canvas.pop_layer();
        canvas
    });
    let mut renderer = NativeRenderer::with_context(&context, 1600, 1030).unwrap();
    for scene in &scenes {
        renderer.render_to_target(scene, target).unwrap().wait().unwrap();
    }
    c.bench_function("native_resize_scratch_capacity", |b| {
        b.iter(|| {
            for scene in &scenes {
                renderer.render_to_target(scene, target).unwrap().wait().unwrap();
            }
        });
    });
}

criterion_group!(benches, resize_with_capacity);
criterion_main!(benches);
