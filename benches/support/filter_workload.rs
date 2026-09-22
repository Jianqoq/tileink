use peniko::{Color, kurbo::Rect};
use tileink::{BlurSampling, Canvas, Filter, Radius, RectLiquidGlass, Region};

pub fn scene(size: (u32, u32)) -> Canvas {
    let mut canvas = Canvas::new(size.0, size.1, 1.0);

    for y in (0..size.1).step_by(32) {
        for x in (0..size.0).step_by(32) {
            let color = if (x / 32 + y / 32) % 2 == 0 {
                Color::from_rgb8(28, 104, 168)
            } else {
                Color::from_rgb8(218, 172, 38)
            };

            canvas.push_rect(
                Rect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from(x + 32),
                    f64::from(y + 32),
                ),
                Radius::ZERO,
                color,
            );
        }
    }

    let bounds = Rect::new(80.0, 64.0, f64::from(size.0 - 80), f64::from(size.1 - 64));

    canvas.push_backdrop_layer(
        Filter::RectLiquidGlass(RectLiquidGlass {
            blur_radius: 10,

            blur_sampling: BlurSampling::FULL_RES,

            tint: Color::TRANSPARENT,

            refraction_factor: 2.0,

            fresnel_factor: 0.0,

            glare_factor: 10.0,

            ..Default::default()
        }),
        Region::rect(bounds, Radius::all(14.0)),
    );

    canvas.push_rect(
        bounds,
        Radius::all(14.0),
        Color::from_rgba8(14, 18, 26, 102),
    );

    canvas.pop_layer();

    canvas
}
