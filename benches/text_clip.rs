use std::{hint::black_box, time::Duration};

use criterion::{Criterion, criterion_group, criterion_main};
use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use tileink::{Canvas, Radius, TextContext, TextFontSystem, TextLayoutOptions, TextWrap};

const CELL_COUNT: usize = 1_000;
const WIDTH: u32 = 1_000;
const HEIGHT: u32 = 40_000;

fn text_clip(c: &mut Criterion) {
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let no_wrap_layout = text_context.layout(
        &mut font_system,
        TextLayoutOptions::new("Long data-grid value that exceeds its cell", 14.0)
            .with_size(Some(152.0), Some(32.0))
            .with_wrap(TextWrap::None),
    );
    let word_or_glyph_layout = text_context.layout(
        &mut font_system,
        TextLayoutOptions::new("Long data-grid value that exceeds its cell", 14.0)
            .with_size(Some(152.0), Some(32.0)),
    );
    if no_wrap_layout.is_empty() || word_or_glyph_layout.is_empty() {
        return;
    }

    let mut group = c.benchmark_group("text_clip_1000_cells");
    group.bench_function("draw_bounds", |b| {
        b.iter(|| {
            let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
            for row in 0..CELL_COUNT {
                let y = row as f64 * 32.0;
                canvas.push_text_layout_clipped(
                    &no_wrap_layout,
                    Point::new(8.0, y + 22.0),
                    Rect::new(8.0, y, 160.0, y + 32.0),
                    Color::BLACK,
                );
            }
            black_box(canvas)
        })
    });
    group.bench_function("word_or_glyph_without_clip", |b| {
        b.iter(|| {
            let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
            for row in 0..CELL_COUNT {
                let y = row as f64 * 32.0;
                canvas.push_text_layout(
                    &word_or_glyph_layout,
                    Point::new(8.0, y + 22.0),
                    Color::BLACK,
                );
            }
            black_box(canvas)
        })
    });
    group.bench_function("rect_clip_layers", |b| {
        b.iter(|| {
            let mut canvas = Canvas::new(WIDTH, HEIGHT, 1.0);
            for row in 0..CELL_COUNT {
                let y = row as f64 * 32.0;
                canvas.push_clip_sdf_rect_layer(Rect::new(8.0, y, 160.0, y + 32.0), Radius::ZERO);
                canvas.push_text_layout(&no_wrap_layout, Point::new(8.0, y + 22.0), Color::BLACK);
                canvas.pop_layer();
            }
            black_box(canvas)
        })
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));
    targets = text_clip
}
criterion_main!(benches);
