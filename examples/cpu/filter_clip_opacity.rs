use crate::common;
use crate::layer_filter_scenes as scenes;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    common::render_to_png(
        "filter_clip_opacity",
        &scenes::filter_clip_opacity_scene(),
        scenes::WIDTH,
        scenes::HEIGHT,
        scenes::CLEAR,
    )
}
