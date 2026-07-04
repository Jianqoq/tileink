use crate::common;
#[path = "../common/sdf_shape_shadows_scene.rs"]
mod sdf_shape_shadows_scene;

use peniko::Color;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_shape_shadows_scene::sdf_shape_shadows_scene();
    common::render_to_png("sdf_shape_shadows", &scene, width, height, Color::WHITE)
}
