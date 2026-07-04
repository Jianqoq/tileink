use crate::common;
#[path = "../common/sdf_line_scene.rs"]
mod sdf_line_scene;

use peniko::Color;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_line_scene::sdf_line_scene();
    common::render_to_png("sdf_line", &scene, width, height, Color::WHITE)
}
