#[path = "../common/candlestick_scene.rs"]
mod candlestick_scene;
#[path = "../common/mod.rs"]
mod common;

use peniko::Color;
use tileink::CubeWgpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = candlestick_scene::candlestick_scene();
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::WHITE);
    renderer.render(&scene);

    let image = renderer.image();
    let out = common::cubecl_example_output("candlestick");
    common::save_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
