#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_rect_shadow_scene.rs"]
mod sdf_rect_shadow_scene;

use peniko::Color;
use tileink::CpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_rect_shadow_scene::sdf_rect_shadow_scene();
    let mut renderer = CpuRenderer::new(width, height, Color::WHITE);
    renderer.render(&scene);

    let out = common::example_output("sdf_rect_shadow");
    common::save_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
