#[path = "../common/mod.rs"]
mod common;
#[path = "../common/sdf_nested_clip_blur_scene.rs"]
mod sdf_nested_clip_blur_scene;

use peniko::Color;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_nested_clip_blur_scene::sdf_nested_clip_blur_scene();
    common::render_to_png_cubecl("sdf_nested_clip_blur", &scene, width, height, Color::WHITE)
}
