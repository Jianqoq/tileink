use cosmic_text::{Buffer, Metrics, Shaping};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tileink::{TextContext, TextFontSystem, TextLayoutOptions};

const TEXT: &str = "Market Watch · ES 6137.71 · +0.63%";

fn text_layout(c: &mut Criterion) {
    let options = TextLayoutOptions::new(TEXT, 12.0)
        .with_line_height(16.0)
        .with_size(Some(280.0), Some(24.0));

    let mut owned_font_system = TextFontSystem::new();
    let mut owned_context = TextContext::new();

    let mut reused_font_system = TextFontSystem::new();
    let mut reused_context = TextContext::new();
    let mut buffer = Buffer::new(&mut reused_font_system, Metrics::new(12.0, 16.0));
    buffer.set_size(Some(280.0), Some(24.0));
    buffer.set_wrap(options.wrap);
    buffer.set_text(
        options.text,
        &options.attrs,
        Shaping::Advanced,
        options.alignment,
    );
    buffer.shape_until_scroll(&mut reused_font_system, false);

    let mut group = c.benchmark_group("text_layout");
    group.bench_function("owned_buffer", |b| {
        b.iter(|| {
            black_box(owned_context.layout(&mut owned_font_system, options.clone()));
        })
    });
    group.bench_function("reuse_shaped_buffer", |b| {
        b.iter(|| {
            black_box(reused_context.layout_buffer(&mut reused_font_system, &mut buffer));
        })
    });
    group.finish();
}

criterion_group!(benches, text_layout);
criterion_main!(benches);
