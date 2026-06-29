#[path = "../common/mod.rs"]
mod common;

#[path = "../common/text_scene.rs"]
mod text_scene;

use tileink::{CpuRenderer, TextContext};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut text_context = TextContext::new();

    for case in &text_scene::CASES {
        let scene = text_scene::scene(&mut text_context, case);
        let mut renderer = CpuRenderer::new(text_scene::WIDTH, text_scene::HEIGHT, case.background);
        renderer.render_with_text(&scene, &mut text_context);
        let out = common::example_output(case.name);
        common::save_image(renderer.image(), &out)?;
        println!("Wrote {}", out.display());
    }

    Ok(())
}
