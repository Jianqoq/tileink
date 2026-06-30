#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_line_scene.rs"]
mod sdf_line_scene;

use peniko::Color;
use tileink::CubeWgpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, _, _) = sdf_line_scene::sdf_line_scene();
    let scene = scene.scaled_to_fit(common::EXAMPLE_WIDTH, common::EXAMPLE_HEIGHT);
    let mut renderer = CubeWgpuRenderer::new_default_device(
        common::EXAMPLE_WIDTH,
        common::EXAMPLE_HEIGHT,
        Color::WHITE,
    );
    renderer.render(&scene);

    let image = renderer.image();
    let out = common::cubecl_example_output("sdf_line");
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
