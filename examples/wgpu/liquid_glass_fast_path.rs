use crate::common::{self, liquid_glass_fast_path as fast_path};
use peniko::Color;
use tileink::Canvas;

// Image certification must not implicitly run profiling loops. Dedicated benchmarks
// remain separate from the shared example catalog and its backend-neutral capture.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    render_scene("liquid_glass_fast_path_mixed", &fast_path::mixed_scene())?;
    render_scene(
        "liquid_glass_fast_path_default",
        &fast_path::single_mode_scene(fast_path::GlassMode::Default),
    )?;
    render_scene(
        "liquid_glass_fast_path_simple",
        &fast_path::single_mode_scene(fast_path::GlassMode::Simple),
    )
}

fn render_scene(name: &str, scene: &Canvas) -> Result<(), Box<dyn std::error::Error>> {
    common::render_to_png_wgpu(
        name,
        scene,
        fast_path::WIDTH,
        fast_path::HEIGHT,
        Color::WHITE,
    )
}
