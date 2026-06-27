#[path = "../common/mod.rs"]
mod common;

#[path = "../common/layer_filter_scenes.rs"]
mod scenes;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    common::render_to_png_cubecl(
        "backdrop_blur",
        &scenes::backdrop_blur_scene(),
        scenes::WIDTH,
        scenes::HEIGHT,
        scenes::CLEAR,
    )
}
