use crate::common;
#[path = "../common/path_current_close_dash_scene.rs"]
mod path_current_close_dash_scene;

use peniko::Color;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (scene, width, height) = path_current_close_dash_scene::path_current_close_dash_scene();
    common::render_to_png_wgpu(
        "path_current_close_dash",
        &scene,
        width,
        height,
        Color::WHITE,
    )
}
