#[path = "../common/mod.rs"]
mod common;

#[path = "../common/gradient_scene.rs"]
mod gradient_scene;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::render_to_png(
        "gradients",
        &gradient_scene::scene(),
        gradient_scene::WIDTH,
        gradient_scene::HEIGHT,
        gradient_scene::CLEAR,
    )
}
