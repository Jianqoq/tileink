#[path = "common/mod.rs"]
mod common;

use std::path::{Path, PathBuf};

use peniko::Color;
use tileink::{CpuRenderer, CubeWgpuRenderer, Scene};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let backend = args
        .next()
        .ok_or("usage: svg_fixture_render <cpu|cubecl> <input.svg> [output.png]")?;
    let input = PathBuf::from(
        args.next()
            .ok_or("usage: svg_fixture_render <cpu|cubecl> <input.svg> [output.png]")?,
    );
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output_path(&input, &backend));

    let data = std::fs::read(&input)?;
    let mut usvg_options = usvg::Options::default();
    usvg_options.resources_dir = input.parent().map(Path::to_path_buf);
    load_svg_test_fonts(&mut usvg_options);
    let tree = usvg::Tree::from_data(&data, &usvg_options)?;
    let size = tree.size();
    let width = size.width().ceil() as u32;
    let height = size.height().ceil() as u32;
    let mut scene = Scene::new(width, height);
    scene.push_svg(&tree)?;

    match backend.as_str() {
        "cpu" => {
            let mut renderer = CpuRenderer::new(width, height, Color::TRANSPARENT);
            renderer.render(&scene);
            common::save_image(renderer.image(), &output)?;
        }
        "cubecl" => {
            let mut renderer =
                CubeWgpuRenderer::new_default_device(width, height, Color::TRANSPARENT);
            renderer.render(&scene);
            common::save_image(&renderer.image(), &output)?;
        }
        _ => return Err(format!("unknown backend `{backend}`, expected cpu or cubecl").into()),
    }

    println!("Wrote {}", output.display());
    Ok(())
}

fn default_output_path(input: &Path, backend: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("svg");
    input.with_file_name(format!("{stem}.{backend}.png"))
}

fn load_svg_test_fonts(options: &mut usvg::Options<'_>) {
    let fonts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("svg")
        .join("fonts");
    options.fontdb_mut().load_fonts_dir(fonts_dir);
}
