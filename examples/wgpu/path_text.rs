#[path = "../common/mod.rs"]
mod common;

#[path = "../common/path_text_scene.rs"]
mod path_text_scene;

use tileink::{TextContext, WgpuRenderer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text_context = TextContext::new();
    let scene = path_text_scene::scene(&mut text_context);
    let mut renderer = WgpuRenderer::new_default_device(
        path_text_scene::WIDTH,
        path_text_scene::HEIGHT,
        path_text_scene::CLEAR,
    );
    renderer.render_with_text(&scene, &mut text_context);

    let image = renderer.image();
    let out = common::wgpu_example_output("path_text");
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
