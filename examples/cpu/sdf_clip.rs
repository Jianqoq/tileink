use crate::common;
#[path = "../common/sdf_clip_scene.rs"]
mod sdf_clip_scene;

use peniko::Color;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_clip_scene::sdf_clip_scene();
    common::render_to_png("sdf_clip", &scene, width, height, Color::WHITE)
}
