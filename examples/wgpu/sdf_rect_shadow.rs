use crate::common;
#[path = "../common/sdf_rect_shadow_scene.rs"]
mod sdf_rect_shadow_scene;

use peniko::Color;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = sdf_rect_shadow_scene::sdf_rect_shadow_scene();
    common::render_to_png_wgpu("sdf_rect_shadow", &scene, width, height, Color::WHITE)
}
