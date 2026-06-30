#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_shape_shadows_scene.rs"]
mod sdf_shape_shadows_scene;

use peniko::Color;
use tileink::CubeWgpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_shape_shadows_scene::sdf_shape_shadows_scene();
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::WHITE);
    renderer.render(&scene);

    let image = renderer.image();
    let out = common::cubecl_example_output("sdf_shape_shadows");
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    O