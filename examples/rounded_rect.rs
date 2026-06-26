mod common;

use peniko::{
    Color,
    kurbo::{Affine, Rect, RoundedRect, Shape, Stroke},
};
use tileink::{CpuRenderer, FillRule, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = 360;
    let height = 240;
    let mut scene = Scene::new(width, height);

    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
    );

    scene.push_path(
        RoundedRect::new(36.0, 34.0, 168.0, 132.0, 24.0).to_path(0.1),
        Color::from_rgb8(54, 151, 118),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    scene.push_path(
        RoundedRect::new(198.0, 34.0, 324.0, 132.0, (8.0, 28.0, 44.0, 18.0)).to_path(0.1),
        Color::from_rgb8(73, 126, 214),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    scene.push_stroke(
        RoundedRect::new(70.0, 158.0, 290.0, 212.0, 18.0),
        Stroke::new(10.0),
        Color::from_rgb8(225, 142, 66),
        Affine::IDENTITY,
        0.1,
    );

    let mut renderer = CpuRenderer::new(width, height, Color::WHITE);
    renderer.render(&scene);

    let out = common::example_output("rounded_rect");
    common::save_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());

    Ok(())
}
