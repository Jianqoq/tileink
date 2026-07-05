use crate::common;

#[path = "../common/emoji_scene.rs"]
mod emoji_scene;

use tileink::{TextContext, TextFontSystem};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let scene = emoji_scene::scene(&mut font_system, &mut text_context);
    common::render_to_png_wgpu_with(
        "emoji",
        emoji_scene::WIDTH,
        emoji_scene::HEIGHT,
        emoji_scene::CLEAR,
        |renderer| {
            renderer.render_with_text(&scene, &mut font_system, &mut text_context);
            Ok(())
        },
    )
}
