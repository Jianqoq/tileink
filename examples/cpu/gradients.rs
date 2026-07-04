use crate::common;

#[path = "../common/gradient_scene.rs"]
mod gradient_scene;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    common::render_to_png(
        "gradients",
        &gradient_scene::scene(),
        gradient_scene::WIDTH,
        gradient_scene::HEIGHT,
        gradient_scene::CLEAR,
    )
}
