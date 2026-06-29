#[path = "../common/mod.rs"]
mod common;

use peniko::Color;

const TARGET_WIDTH: u32 = 900;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = common::example_asset("tiger.svg");
    let (scene, width, height) = common::load_svg_scene(input, TARGET_WIDTH)?;
    common::render_to_png_cubecl("svg_tiger", &scene, width, height, Color::TRANSPARENT)
}
