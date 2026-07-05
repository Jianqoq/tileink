use crate::common;

#[path = "../common/text_scene.rs"]
mod text_scene;

use tileink::{TextContext, TextFontSystem};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    for case in &text_scene::CASES {
        let scene = text_scene::scene(&mut font_system, &mut text_context, case);
        common::render_to_png_wgpu_with(
            case.name,
            text_scene::WIDTH,
            text_scene::HEIGHT,
            case.background,
            |renderer| {
                renderer.render_with_text(&scene, &mut font_system, &mut text_context);
                Ok(())
            },
        )?;
    }

    Ok(())
}
