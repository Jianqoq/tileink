use crate::common;

#[path = "../common/path_text_scene.rs"]
mod path_text_scene;

use tileink::{TextContext, TextFontSystem, WgpuRenderer};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let scene = path_text_scene::scene(&mut font_system, &mut text_context);
    let mut renderer = WgpuRenderer::new_default_device(
        path_text_scene::WIDTH,
        path_text_scene::HEIGHT,
        path_text_scene::CLEAR,
    );
    renderer.render_with_text(&scene, &mut font_system, &mut text_context);

    let image = renderer.image();
    let out = common::wgpu_example_output("path_text");
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
