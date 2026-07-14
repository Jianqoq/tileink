use std::{hint::black_box, rc::Rc, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use peniko::{Color, kurbo::Affine};
use tileink::{
    Canvas, Radius, RetainedMaterializerBenchmark, RetainedNodeId, RetainedParent, RetainedScene,
};

fn materializer(draws: usize) -> (RetainedMaterializerBenchmark, RetainedNodeId) {
    let root = RetainedNodeId::new(1, 1);
    let node = RetainedNodeId::new(2, 1);
    let mut canvas = Canvas::new(2048, 2048, 1.0);
    for index in 0..draws {
        let x = (index % 128) as f64 * 16.0;
        let y = (index / 128) as f64 * 16.0;
        canvas.push_rect(
            peniko::kurbo::Rect::new(x, y, x + 12.0, y + 12.0),
            Radius::ZERO,
            Color::BLACK,
        );
    }
    let mut scene = RetainedScene::new(2048, 2048, 1.0, root).unwrap();
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            Rc::new(canvas),
            Affine::IDENTITY,
        )
        .commit()
        .unwrap();
    (RetainedMaterializerBenchmark::new(&scene), node)
}

fn node_draw_order(c: &mut Criterion) {
    let cases = [
        ("one", 1, 1),
        ("64", 64, 1),
        ("64-repeated-8", 64, 8),
        ("1024", 1024, 1),
        ("1024-repeated-8", 1024, 8),
    ];
    let mut group = c.benchmark_group("node_draw_order");
    for (name, draws, repetitions) in cases {
        let (materializer, node) = materializer(draws);
        group.throughput(Throughput::Elements((draws * repetitions) as u64));
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            b.iter(|| {
                black_box(
                    materializer.visit_node_physical_draws(black_box(node), black_box(repetitions)),
                )
            });
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = node_draw_order
}
criterion_main!(benches);
