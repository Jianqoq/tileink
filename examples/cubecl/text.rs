#[path = "../common/mod.rs"]
mod common;

#[path = "../common/text_scene.rs"]
mod text_scene;

use tileink::{CubeWgpuRenderer, TextContext};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text_context = TextContext::new();

    for case in &text_scene::CASES {
        let scene = text_scene::scene(&mut text_context, case);
        let mut renderer = CubeWgpuRenderer::new_default_device(
            text_scene::WIDTH,
            text_scene::HEIGHT,
            case.background,
        );
        renderer.render_with_text(&scene, &mut text_context);
        let image = renderer.image();
        let out = common::cubecl_example_output(case.name);
        common::save_example_image(&image, &out)?;
        println!("Wrote {}", out.display());
    }

    Ok(())
}
