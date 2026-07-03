#[path = "../common/mod.rs"]
mod common;
#[path = "../common/path_current_close_dash_scene.rs"]
mod path_current_close_dash_scene;

use peniko::Color;
use tileink::WgpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = path_current_close_dash_scene::path_current_close_dash_scene();
    let mut renderer = WgpuRenderer::new_default_device(width, height, Color::WHITE);
    renderer.render(&scene);

    let image = renderer.image();
    let out = common::wgpu_example_output("path_current_close_dash");
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
