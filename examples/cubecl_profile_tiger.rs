#[path = "common/mod.rs"]
mod common;

use peniko::Color;
use tileink::{CubeWgpuRenderer, RenderProfileReport};

const TARGET_WIDTH: u32 = 900;
const WARMUP: usize = 3;
const ITERATIONS: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = common::example_asset("tiger.svg");
    let (scene, width, height) = common::load_svg_scene(input, TARGET_WIDTH)?;
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::TRANSPARENT);

    for _ in 0..WARMUP {
        renderer.render(&scene);
    }

    println!("tiger profile: {width}x{height}, warmup={WARMUP}, iterations={ITERATIONS}");
    let mut report = RenderProfileReport::new();
    for _ in 0..ITERATIONS {
        renderer.start_profile();
        renderer.render(&scene);
        report.push(renderer.end_profile());
    }
    println!("{report}");

    Ok(())
}
