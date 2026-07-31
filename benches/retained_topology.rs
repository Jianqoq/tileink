use std::{hint::black_box, rc::Rc, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use tileink::{
    Canvas, Radius, RetainedMaterializerBenchmark, RetainedNodeId, RetainedParent, RetainedScene,
};

const COUNTS: [usize; 3] = [100, 1_000, 10_000];
const ROOT: RetainedNodeId = RetainedNodeId::new(u64::MAX, 0);
const TOGGLED: RetainedNodeId = RetainedNodeId::new(u64::MAX - 1, 0);

fn leaf_canvas() -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 64, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 1.0, 1.0), Radius::ZERO, Color::WHITE);
    Rc::new(canvas)
}

fn scene_with_leaves(count: usize, leaf: &Rc<Canvas>) -> RetainedScene {
    let mut scene = RetainedScene::new(64, 64, 1.0, ROOT).unwrap();
    let mut transaction = scene.transaction();
    for index in 0..count {
        transaction.insert_scene(
            RetainedParent::content(ROOT),
            None,
            RetainedNodeId::new(index as u64 + 1, 0),
            leaf.clone(),
            Affine::IDENTITY,
        );
    }
    transaction.commit().unwrap();
    scene
}

fn retained_topology(c: &mut Criterion) {
    let leaf = leaf_canvas();
    let mut group = c.benchmark_group("retained_topology/plain-leaf-tail-toggle");
    group.throughput(Throughput::Elements(1));
    for count in COUNTS {
        let mut scene = scene_with_leaves(count, &leaf);
        let mut materializer = RetainedMaterializerBenchmark::new(&scene);
        let mut inserted = false;
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                let mut transaction = scene.transaction();
                if inserted {
                    transaction.remove_subtree(TOGGLED);
                } else {
                    transaction.insert_scene(
                        RetainedParent::content(ROOT),
                        None,
                        TOGGLED,
                        leaf.clone(),
                        Affine::IDENTITY,
                    );
                }
                transaction.commit().unwrap();
                inserted = !inserted;
                black_box(materializer.update_incremental(black_box(&scene)))
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
    targets = retained_topology
}
criterion_main!(benches);
