use crate::common;

#[path = "../common/path_text_scene.rs"]
mod path_text_scene;

use tileink::{CpuRenderer, TextContext, TextFontSystem};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let scene = path_text_scene::scene(&mut font_system, &mut text_context);
    let mut renderer = CpuRenderer::new(
        path_text_scene::WIDTH,
        path_text_scene::HEIGHT,
        path_text_scene::CLEAR,
    );
    renderer.render_with_text(&scene, &mut font_system, &mut text_context);

    let out = common::example_output("path_text");
    common::save_example_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
