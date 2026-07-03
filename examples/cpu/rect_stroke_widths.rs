#[path = "../common/mod.rs"]
mod common;

use peniko::{Color, kurbo::Rect};
use tileink::{CpuRenderer, Radius, Canvas, StrokeWidths};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = 480;
    let height = 320;
    let mut scene = Canvas::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        tileink::Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    scene.push_rect(
        Rect::new(24.0, 24.0, 456.0, 296.0),
        tileink::Radius::ZERO,
        Color::from_rgb8(235, 239, 244),
    );

    scene.push_rect_stroke_widths(
        Rect::new(58.0, 58.0, 210.0, 150.0),
        Radius::ZERO,
        StrokeWidths {
            top: 4.0,
            right: 22.0,
            bottom: 12.0,
            left: 34.0,
        },
        Color::from_rgb8(220, 64, 72),
    );

    scene.push_rect_stroke_widths(
        Rect::new(282.0, 54.0, 424.0, 154.0),
        Radius::all(22.0),
        StrokeWidths {
            top: 18.0,
            right: 6.0,
            bottom: 28.0,
            left: 12.0,
        },
        Color::from_rgb8(49, 112, 214),
    );

    scene.push_rect_stroke_widths(
        Rect::new(92.0, 204.0, 388.0, 258.0),
        Radius::all(10.0),
        StrokeWidths {
            top: 8.0,
            right: 32.0,
            bottom: 8.0,
            left: 16.0,
        },
        Color::from_rgb8(28, 153, 104),
    );

    let mut renderer = CpuRenderer::new(width, height, Color::from_rgb8(255, 255, 255));
    renderer.render(&scene);

    let out = common::example_output("rect_stroke_widths");
    common::save_example_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());

    Ok(())
}
