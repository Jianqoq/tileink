#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_shape_shadows_scene.rs"]
mod sdf_shape_shadows_scene;

use peniko::Color;
use tileink::CpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_shape_shadows_scene::sdf_shape_shadows_scene();
    let mut renderer = CpuRenderer::new(width, height, Color::WHITE);
    renderer.render(&scene);

    let out = common::example_output("sdf_shape_shadows");
    common::save_example_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
