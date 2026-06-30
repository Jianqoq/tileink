#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_rect_shadow_scene.rs"]
mod sdf_rect_shadow_scene;

use peniko::Color;
use tileink::CubeWgpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_rect_shadow_scene::sdf_rect_shadow_scene();
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::WHITE);
    renderer.render(&scene);

    let image = renderer.image();
    let out = common::cubecl_example_output("sdf_rect_shadow");
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
