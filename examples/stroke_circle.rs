mod common;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Stroke},
};
use tileink::{CpuRenderer, FillRule, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = 220;
    let height = 180;
    let mut scene = Scene::new(width, height);

    scene.push_rect(
        peniko::kurbo::Rect::new(0.0, 0.0, width as f64, height as f64),
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

    let mut renderer = CpuRenderer::new(width, height, Color::WHITE);
    renderer.render(&scene);

    let out = common::example_output("stroke_circle");
    common::save_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());

    Ok(())
}
