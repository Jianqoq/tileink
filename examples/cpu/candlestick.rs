#[path = "../common/candlestick_scene.rs"]
mod candlestick_scene;
#[path = "../common/mod.rs"]
mod common;

use peniko::Color;
use tileink::CpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, _, _) = candlestick_scene::candlestick_scene();
    let scene = scene.scaled_to_fit(common::EXAMPLE_WIDTH, common::EXAMPLE_HEIGHT);
    let mut renderer =
        CpuRenderer::new(common::EXAMPLE_WIDTH, common::EXAMPLE_HEIGHT, Color::WHITE);
    renderer.render(&scene);

    let out = common::example_output("candlestick");
    common::save_example_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
