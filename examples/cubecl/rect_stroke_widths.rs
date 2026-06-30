#[path = "../common/mod.rs"]
mod common;

use peniko::{Color, kurbo::Rect};
use tileink::{CubeWgpuRenderer, FillRule, Radius, Scene, StrokeWidths};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(480, 320);

    scene.push_rect(
        Rect::new(0.0, 0.0, 480.0, 320.0),
        tileink::Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(24.0, 24.0, 456.0, 296.0),
        tileink::Radius::ZERO,
        Color::from_rgb8(235, 239, 244),
        FillRule::NonZero,
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
        FillRule::NonZero,
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
        FillRule::NonZero,
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
        FillRule::NonZero,
    );

    let mut renderer =
        CubeWgpuRenderer::new_default_device(480, 320, Color::from_rgb8(255, 255, 255));
    renderer.render(&scene);

    let out = common::cubecl_example_output("rect_stroke_widths");
    let image = renderer.image();
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());

    Ok(())
}
