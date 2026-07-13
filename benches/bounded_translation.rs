use std::{hint::black_box, rc::Rc, time::Duration};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use tileink::{
    Canvas, Radius, RetainedMaterializerBenchmark, RetainedNodeId, RetainedParent, RetainedScene,
};

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 1024;
const CHUNKS: usize = 32;
const DRAWS_PER_CHUNK: usize = 32;

fn node_id(index: usize) -> RetainedNodeId {
    RetainedNodeId::new(index as u64 + 1, 1)
}

fn chart_chunk() -> Rc<Canvas> {
    let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
    for index in 0..DRAWS_PER_CHUNK {
        let x = (index % 16) as f64 * 12.0;
        let y = (index / 16) as f64 * 20.0;
        canvas.push_rect(
            Rect::new(x, y, x + 8.0, y + 14.0),
            Radius::ZERO,
            if index.is_multiple_of(2) {
                Color::from_rgb8(38, 166, 154)
            } else {
                Color::from_rgb8(239, 83, 80)
            },
        );
    }
    Rc::new(canvas)
}

fn chart_scene(bounded: bool) -> RetainedScene {
    let root = RetainedNodeId::new(u64::MAX, 0);
    let mut scene = RetainedScene::new(WIDTH, HEIGHT, 1.0, root).unwrap();
    let chunk = chart_chunk();
    let damage = Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64);
    let mut transaction = scene.transaction();
    for index in 0..CHUNKS {
        let transform = Affine::translate(((index % 8) as f64 * 24.0, (index / 8) as f64 * 16.0));
        if bounded {
            transaction.insert_bounded_scene(
                RetainedParent::content(root),
                None,
                node_id(index),
                chunk.clone(),
                transform,
                damage,
            );
        } else {
            transaction.insert_scene(
                RetainedParent::content(root),
                None,
                node_id(index),
                chunk.clone(),
                transform,
            );
        }
    }
    transaction.commit().unwrap();
    scene
}

fn mutate_chart(scene: &mut RetainedScene, frame: usize, bounded: bool) {
    let offset = if frame.is_multiple_of(2) { 0.0 } else { 16.0 };
    let damage = Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64);
    let mut transaction = scene.transaction();
    for index in 0..CHUNKS {
        let transform = Affine::translate((
            (index % 8) as f64 * 24.0 + offset,
            (index / 8) as f64 * 16.0,
        ));
        if bounded {
            transaction.set_bounded_translation(node_id(index), transform, damage);
        } else {
            transaction.set_transform(node_id(index), transform);
        }
    }
    transaction.commit().unwrap();
}

fn bounded_translation(c: &mut Criterion) {
    let mut group = c.benchmark_group("bounded_translation/cpu_update");
    group.throughput(Throughput::Elements((CHUNKS * DRAWS_PER_CHUNK) as u64));
    for (name, bounded) in [("ordinary", false), ("fixed-domain", true)] {
        let mut scene = chart_scene(bounded);
        let mut materializer = RetainedMaterializerBenchmark::new(&scene);
        let mut frame = 0usize;
        group.bench_function(BenchmarkId::new(name, CHUNKS), |b| {
            b.iter(|| {
                frame = frame.wrapping_add(1);
                mutate_chart(&mut scene, frame, bounded);
                black_box(materializer.update(black_box(&scene)))
            });
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = bounded_translation
}
criterion_main!(benches);
