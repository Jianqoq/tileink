mod common;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect, Shape, Stroke},
};
use tileink::{CpuRenderer, FillRule, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Scene::new(360, 260);

    scene.push_rect(
        Rect::new(0.0, 0.0, 360.0, 260.0),
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
    );
    scene.push_rect(
        Rect::new(42.0, 38.0, 178.0, 128.0),
        Color::from_rgba8(37, 143, 93, 230),
        FillRule::NonZero,
    );
    scene.push_path(
        Circle::new((242.0, 86.0), 56.0).to_path(0.1),
        Color::from_rgba8(45, 111, 211, 220),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    scene.push_stroke(
        Rect::new(72.0, 156.0, 178.0, 218.0),
        Stroke::new(12.0),
        Color::from_rgb8(230, 89, 80),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        Circle::new((260.0, 184.0), 40.0),
        Stroke::new(10.0),
        Color::from_rgb8(222, 178, 106),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    let mut renderer = CpuRenderer::new(360, 260, Color::from_rgb8(255, 255, 255));
    renderer.render(&scene);

    let out = common::example_output("simple");
    common::save_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());

    Ok(())
}
