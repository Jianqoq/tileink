use crate::common;

#[path = "../common/emoji_scene.rs"]
mod emoji_scene;

use tileink::{TextContext, TextFontSystem, WgpuRenderer};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let scene = emoji_scene::scene(&mut font_system, &mut text_context);
    let mut renderer = WgpuRenderer::new_default_device(
        emoji_scene::WIDTH,
        emoji_scene::HEIGHT,
        emoji_scene::CLEAR,
    );
    renderer.render_with_text(&scene, &mut font_system, &mut text_context);

    let image = renderer.image();
    let out = common::wgpu_example_output("emoji");
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
