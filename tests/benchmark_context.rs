use retained_bench::benchmark_gpu;

#[allow(dead_code)]
#[path = "../examples/support/retained_bench.rs"]
mod retained_bench;

use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
use tileink::{Canvas, Radius, RetainedNodeId, RetainedParent, RetainedScene};

#[test]
fn wgpu_benchmark_context_keeps_new_renderers_independent() {
    if std::env::var("TILEINK_RUN_WGPU_TESTS").as_deref() != Ok("1") {
        return;
    }
    let portable = match std::env::var("TILEINK_WGPU_MODE").as_deref() {
        Ok("portable") => true,
        Ok("native") | Err(std::env::VarError::NotPresent) => false,
        _ => panic!("TILEINK_WGPU_MODE must be native or portable"),
    };
    let (device, queue) = match std::env::var("TILEINK_BENCH_API") {
        Ok(api) => {
            // The measured route and regression test use the same physical GPU and DXC selector.
            let (_, device, queue) =
                benchmark_gpu::device(&api, portable, false, wgpu::MemoryHints::MemoryUsage);
            (device, queue)
        }
        Err(std::env::VarError::NotPresent) => {
            assert!(
                std::env::var_os("TILEINK_BENCH_GPU").is_none(),
                "a pinned physical GPU also requires TILEINK_BENCH_API"
            );
            benchmark_gpu::default_device(Some(portable))
        }
        Err(error) => panic!("invalid TILEINK_BENCH_API: {error}"),
    };
    let context = retained_bench::BenchContext::new(&device, &queue);
    assert_eq!(
        context.shared_pipeline_cache_present(),
        device.features().contains(wgpu::Features::PIPELINE_CACHE),
    );
    let root = RetainedNodeId::for_owner(90_000);
    let node = RetainedNodeId::for_owner(90_001);
    let mut scene = RetainedScene::new(35, 19, 1.0, root).unwrap();
    let leaf = |color| {
        let mut canvas = Canvas::new(35, 19, 1.0);
        canvas.push_rect(Rect::new(1.0, 1.0, 10.0, 10.0), Radius::ZERO, color);
        Rc::new(canvas)
    };
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            leaf(Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    let mut first = context.renderer();
    first.render_retained(&scene);
    let first_image = first.image();
    assert_eq!((first_image.width, first_image.height), (35, 19));
    let white = first_image.pixels;
    let mut second = context.renderer();
    second.render_retained(&scene);
    assert_eq!(second.image().pixels, white);

    scene
        .transaction()
        .replace_scene(node, leaf(Color::BLACK))
        .commit()
        .unwrap();
    first.render_retained(&scene);
    let black = first.image().pixels;
    assert_ne!(black, white);
    assert_eq!(
        second.image().pixels,
        white,
        "a shared pipeline cache must not alias output state"
    );
    second.render_retained(&scene);
    assert_eq!(
        second.image().pixels,
        black,
        "each renderer must independently consume the scene journal"
    );
    for (index, pixel) in black.iter().enumerate() {
        let (x, y) = (index % 35, index / 35);
        let expected = if (1..10).contains(&x) && (1..10).contains(&y) {
            0xff000000
        } else {
            0
        };
        assert_eq!(*pixel, expected);
    }

    // A Criterion session must retain its mutation cursor across samples,
    // including empty samples, rather than rebuilding and warming each time.
    let mut session = retained_bench::PersistentSession::new(
        &context,
        scene,
        tileink::IncrementalRenderConfig::default(),
        false,
    )
    .unwrap();
    let mut observed = Vec::new();
    let mut mutate = |scene: &mut RetainedScene, frame: usize| {
        observed.push(frame);
        scene
            .transaction()
            .invalidate_rect(Rect::new(0.0, 0.0, 8.0, 8.0))
            .commit()
            .unwrap();
    };
    session.warm(3, &mut mutate).unwrap();
    assert_eq!(session.measure(2, &mut mutate).unwrap().wall.len(), 2);
    assert!(session.measure(0, &mut mutate).unwrap().wall.is_empty());
    assert_eq!(session.measure(2, &mut mutate).unwrap().wall.len(), 2);
    assert_eq!(observed, (0..7).collect::<Vec<_>>());
}
