#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_shape_shadows_scene.rs"]
mod sdf_shape_shadows_scene;

use peniko::Color;
use tileink::CpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, _, _) = sdf_shape_shadows_scene::sdf_shape_shadows_scene();
    let scene = scene.scaled_to_fit(common::EXAMPLE_WIDTH, common::EXAMPLE_HEIGHT);
    let mut renderer =
        CpuRenderer::new(common::EXAMPLE_WIDTH, common::EXAMPLE_HEIGHT, Color::WHITE);
    renderer.render(&scene);

    let out = common::example_output("sdf_shape_shadows");
    common::save_example_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
