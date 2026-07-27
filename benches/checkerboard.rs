use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use peniko::{Color, kurbo::Rect};
use tileink::{Canvas, Radius};

const WIDTH: f64 = 16.0;
const HEIGHT: f64 = 240.0;
const CELL: f64 = 8.0;

fn checkerboard_recording(c: &mut Criterion) {
    let mut group = c.benchmark_group("checkerboard_recording");
    group.bench_function("analytic_two_draw_sdf", |b| {
        b.iter(|| {
            let mut canvas = Canvas::new(WIDTH as u32, HEIGHT as u32, 1.0);
            canvas
                .push_checkerboard(
                    Rect::new(0.0, 0.0, WIDTH, HEIGHT),
                    CELL as f32,
                    Color::from_rgb8(194, 194, 194),
                    Color::WHITE,
                )
                .unwrap();
            black_box(canvas.draw_count())
        });
    });
    group.bench_function("per_cell_sdf_rects", |b| {
        b.iter(|| {
            let mut canvas = Canvas::new(WIDTH as u32, HEIGHT as u32, 1.0);
            for row in 0..(HEIGHT / CELL) as u32 {
                for col in 0..(WIDTH / CELL) as u32 {
                    let color = if (row + col) % 2 == 0 {
                        Color::from_rgb8(194, 194, 194)
                    } else {
                        Color::WHITE
                    };
                    canvas.push_rect(
                        Rect::new(
                            f64::from(col) * CELL,
                            f64::from(row) * CELL,
                            f64::from(col + 1) * CELL,
                            f64::from(row + 1) * CELL,
                        ),
                        Radius::ZERO,
                        color,
                    );
                }
            }
            black_box(canvas.draw_count())
        });
    });
    group.finish();
}

criterion_group!(benches, checkerboard_recording);
criterion_main!(benches);
