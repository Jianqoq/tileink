#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_line_scene.rs"]
mod sdf_line_scene;

use peniko::Color;
use tileink::CubeWgpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_line_scene::sdf_line_scene();
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::WHITE);
    renderer.render(&scene);

    let image = renderer.image();
    let out = common::cubecl_example_output("sdf_line");
    common::save_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
