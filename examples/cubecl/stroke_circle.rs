#[path = "../common/mod.rs"]
mod common;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Stroke},
};
use tileink::{CubeWgpuRenderer, FillRule, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = 220;
    let height = 180;
    let mut scene = Scene::new(width, height);

    scene.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
        tileink::Radius::ZERO,
        Color::from_rgb8(250, 250, 248),
        FillRule::NonZero,
    );
    scene.push_stroke(
        Circle::new((110.0, 90.0), 54.0),
        Stroke::new(16.0),
        Color::from_rgb8(222, 96, 72),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    let scene = scene.scaled_to_fit(common::EXAMPLE_WIDTH, common::EXAMPLE_HEIGHT);
    let mut renderer = CubeWgpuRenderer::new_default_device(
        common::EXAMPLE_WIDTH,
        common::EXAMPLE_HEIGHT,
        Color::WHITE,
    );
    renderer.render(&scene);

    let out = common::cubecl_example_output("stroke_circle");
    let image = renderer.image();
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());

    Ok(())
}
