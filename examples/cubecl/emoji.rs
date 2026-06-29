#[path = "../common/mod.rs"]
mod common;

#[path = "../common/emoji_scene.rs"]
mod emoji_scene;

use tileink::{CubeWgpuRenderer, TextContext};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text_context = TextContext::new();
    let scene = emoji_scene::scene(&mut text_context);
    let mut renderer = CubeWgpuRenderer::new_default_device(
        emoji_scene::WIDTH,
        emoji_scene::HEIGHT,
        emoji_scene::CLEAR,
    );
    renderer.render_with_text(&scene, &mut text_context);

    let image = renderer.image();
    let out = common::cubecl_example_output("emoji");
    common::save_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}
