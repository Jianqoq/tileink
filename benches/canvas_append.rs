use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use tileink::{Canvas, Radius};

const DRAWS: usize = 256;

fn child_scene() -> Canvas {
    let mut canvas = Canvas::new(512, 512, 1.0);
    for index in 0..DRAWS {
        let x = (index % 16) as f64 * 24.0;
        let y = (index / 16) as f64 * 24.0;
        canvas.push_rect(
            Rect::new(x, y, x + 18.0, y + 18.0),
            Radius::all(4.0),
            Color::from_rgb8(35, 108, 235),
        );
    }
    canvas
}

fn canvas_append(c: &mut Criterion) {
    let child = child_scene();
    let mut group = c.benchmark_group("canvas_append");
    group.throughput(Throughput::Elements(DRAWS as u64));
    group.bench_function("native-translation", |b| {
        b.iter_batched(
            || Canvas::new(1024, 768, 1.0),
            |mut target| {
                target.append(&child, (32.0, 24.0));
                black_box(target)
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("append-transformed-translation", |b| {
        b.iter_batched(
            || Canvas::new(1024, 768, 1.0),
            |mut target| {
                target.append_transformed(&child, Affine::translate((32.0, 24.0)));
                black_box(target)
            },
            BatchSize::SmallInput,
        )
    });
    group.bench_function("reused-target-translation", |b| {
        let mut target = Canvas::new(1024, 768, 1.0);
        b.iter(|| {
            target.reset_for_surface(1024, 768, 1.0);
            target.append_transformed(&child, Affine::translate((32.0, 24.0)));
            black_box(target.draw_count())
        })
    });
    group.bench_function("general-affine", |b| {
        b.iter_batched(
            || Canvas::new(1024, 768, 1.0),
            |mut target| {
                target.append_transformed(
                    &child,
                    Affine::translate((32.0, 24.0)) * Affine::rotate(0.05),
                );
                black_box(target)
            },
            BatchSize::SmallInput,
        )
    });
    group.finish();
}

criterion_group!(benches, canvas_append);
criterion_main!(benches);
