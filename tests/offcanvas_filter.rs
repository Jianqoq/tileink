#[allow(dead_code, unused_imports)]
#[path = "../benches/support/backend_comparison.rs"]
mod backend;

#[test]
#[ignore = "requires pinned TILEINK_BENCH_GPU and TILEINK_BENCH_API"]
fn offset_filter_preserves_offcanvas_input() {
    use peniko::{Color, kurbo::Rect};
    use tileink::{Canvas, Filter, Radius, Region};
    let mut gpu = backend::Gpu::new();
    let target = gpu.target(32, 32);
    for (source, dx, dy, output) in [
        (
            Rect::new(-16.0, 0.0, -8.0, 8.0),
            16.0,
            0.0,
            Rect::new(0.0, 0.0, 8.0, 8.0),
        ),
        (
            Rect::new(40.0, 0.0, 48.0, 8.0),
            -16.0,
            0.0,
            Rect::new(24.0, 0.0, 32.0, 8.0),
        ),
        (
            Rect::new(0.0, -16.0, 8.0, -8.0),
            0.0,
            16.0,
            Rect::new(0.0, 0.0, 8.0, 8.0),
        ),
        (
            Rect::new(0.0, 40.0, 8.0, 48.0),
            0.0,
            -16.0,
            Rect::new(0.0, 24.0, 8.0, 32.0),
        ),
    ] {
        let mut canvas = Canvas::new(32, 32, 1.0);
        canvas.push_filter_layer(
            Filter::Offset { dx, dy },
            Region::rect(source, Radius::ZERO),
        );
        canvas.push_rect(source, Radius::ZERO, Color::WHITE);
        canvas.pop_layer();
        gpu.render_immediate(&canvas, &target);
        let image = gpu.image_target(&target);
        for y in 0..32 {
            for x in 0..32 {
                let expected = if output.contains((f64::from(x) + 0.5, f64::from(y) + 0.5)) {
                    u32::MAX
                } else {
                    0
                };
                assert_eq!(
                    image.pixels[y as usize * 32 + x as usize],
                    expected,
                    "offset ({dx},{dy}), pixel ({x},{y})"
                );
            }
        }
    }
}
