use peniko::Color;
use tileink::{CpuRenderer, Scene};

#[path = "../common/mod.rs"]
mod common;

#[path = "../common/liquid_glass_fast_path.rs"]
mod fast_path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    render_scene("liquid_glass_fast_path_mixed", &fast_path::mixed_scene())?;
    render_scene(
        "liquid_glass_fast_path_default",
        &fast_path::single_mode_scene(fast_path::GlassMode::Default),
    )?;
    render_scene(
        "liquid_glass_fast_path_simple",
        &fast_path::single_mode_scene(fast_path::GlassMode::Simple),
    )?;
    Ok(())
}

fn render_scene(name: &str, scene: &Scene) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = CpuRenderer::new(fast_path::WIDTH, fast_path::HEIGHT, Color::WHITE);
    renderer.render(scene);
    let out = common::example_output(name);
    common::save_example_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
