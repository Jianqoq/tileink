#[path = "../common/mod.rs"]
mod common;

#[path = "../common/svg_radial_gradient_scene.rs"]
mod svg_radial_gradient_scene;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scene = svg_radial_gradient_scene::scene()?;
    common::render_to_png_wgpu(
        "svg_radial_gradient",
        &scene,
        svg_radial_gradient_scene::WIDTH,
        svg_radial_gradient_scene::HEIGHT,
        svg_radial_gradient_scene::CLEAR,
    )
}
