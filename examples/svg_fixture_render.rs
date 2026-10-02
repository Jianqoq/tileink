//! Render SVG fixtures through the selected native backend.
#[path = "common/mod.rs"]
mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};
use tileink::{NativeBackend, NativeContext, NativeContextOptions, NativeRenderer};

fn backend() -> Result<NativeBackend, Box<dyn std::error::Error>> {
    #[cfg(feature = "dx12")]
    {
        return Ok(NativeBackend::Dx12);
    }
    #[cfg(feature = "vulkan")]
    {
        return Ok(NativeBackend::Vulkan);
    }
    #[cfg(feature = "metal")]
    {
        return Ok(NativeBackend::Metal);
    }
    #[allow(unreachable_code)]
    Err("enable one native backend feature".into())
}

fn collect_svg_files(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_svg_files(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args()
        .nth(1)
        .ok_or("usage: svg_fixture_render <folder>")?;
    let root = Path::new(&root);
    if !root.is_dir() {
        return Err(format!("input must be a folder: {}", root.display()).into());
    }
    let context = NativeContext::new(backend()?, &NativeContextOptions::default())?;
    let mut files = Vec::new();
    collect_svg_files(root, &mut files)?;
    files.sort();
    let mut failures = Vec::new();
    for input in files {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let (scene, width, height) = common::load_svg_scene(&input, 300)?;
            let mut renderer = NativeRenderer::with_context(&context, width, height)?;
            let image = renderer.render_to_image(&scene)?.readback()?;
            let output = input.with_file_name(format!(
                "{}.native.png",
                input.file_stem().unwrap().to_string_lossy()
            ));
            image.save(&output)?;
            Ok(())
        })();
        if let Err(error) = result {
            failures.push(format!("{}: {error}", input.display()));
        }
    }
    if !failures.is_empty() {
        for failure in &failures {
            eprintln!("{failure}");
        }
        return Err(format!("{} SVG fixtures failed", failures.len()).into());
    }
    Ok(())
}
