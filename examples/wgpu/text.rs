use crate::common;

#[path = "../common/text_scene.rs"]
mod text_scene;

use tileink::{TextContext, TextFontSystem, WgpuRenderer};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let mut renderer = WgpuRenderer::new_default_device(
        text_scene::WIDTH,
        text_scene::HEIGHT,
        text_scene::CASES[0].background,
    );

    for case in &text_scene::CASES {
        let scene = text_scene::scene(&mut font_system, &mut text_context, case);
        renderer.set_clear_color(case.background);
        renderer.render_with_text(&scene, &mut font_system, &mut text_context);
        let image = renderer.image();
        let out = common::wgpu_example_output(case.name);
        common::save_example_image(&image, &out)?;
        println!("Wrote {}", out.display());
    }

    Ok(())
}
