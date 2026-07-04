use crate::common;
#[path = "../common/sdf_nested_clip_blur_scene.rs"]
mod sdf_nested_clip_blur_scene;

use peniko::Color;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_nested_clip_blur_scene::sdf_nested_clip_blur_scene();
    common::render_to_png("sdf_nested_clip_blur", &scene, width, height, Color::WHITE)
}
