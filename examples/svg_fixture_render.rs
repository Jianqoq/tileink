#[path = "common/mod.rs"]
mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use peniko::Color;
use tileink::{CpuRenderer, CubeWgpuRenderer, Scene};

#[derive(Clone, Copy)]
enum Backend {
    Both,
    Cpu,
    CubeCl,
}

impl Backend {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "both" => Ok(Self::Both),
            "cpu" => Ok(Self::Cpu),
            "cubecl" => Ok(Self::CubeCl),
            _ => Err(format!(
                "unknown backend `{value}`, expected both, cpu, or cubecl"
            )),
        }
    }

    fn renders_cpu(self) -> bool {
        matches!(self, Self::Both | Self::Cpu)
    }

    fn renders_cubecl(self) -> bool {
        matches!(self, Self::Both | Self::CubeCl)
    }
}

struct BatchRenderers {
    cpu: Option<CpuRenderer>,
    cubecl: Option<CubeWgpuRenderer>,
}

impl BatchRenderers {
    fn new(backend: Backend) -> Self {
        Self {
            cpu: backend
                .renders_cpu()
                .then(|| CpuRenderer::new(1, 1, Color::TRANSPARENT)),
            cubecl: backend
                .renders_cubecl()
                .then(|| CubeWgpuRenderer::new_default_device(1, 1, Color::TRANSPARENT)),
        }
    }

    fn render_cpu(
        &mut self,
        scene: &Scene,
        input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(renderer) = &mut self.cpu else {
            return Ok(());
        };
        let output = output_path(input, "cpu");
        renderer.render(scene);
        common::save_image(renderer.image(), &output)?;
        println!("[cpu] wrote {}", output.display());
        Ok(())
    }

    fn render_cubecl(
        &mut self,
        scene: &Scene,
        input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(renderer) = &mut self.cubecl else {
            return Ok(());
        };
        let output = output_path(input, "cubecl");
        renderer.render(scene);
        common::save_image(&renderer.image(), &output)?;
        println!("[cubecl] wrote {}", output.display());
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("usage: svg_fixture_render <folder> [both|cpu|cubecl]")?,
    );
    let backend = args
        .next()
        .map(|value| Backend::parse(&value))
        .transpose()?
        .unwrap_or(Backend::Both);

    if !root.is_dir() {
        return Err(format!("input must be a folder: {}", root.display()).into());
    }

    let mut svgs = Vec::new();
    collect_svg_files(&root, &mut svgs)?;
    svgs.sort();

    let mut usvg_options = usvg::Options::default();
    load_svg_test_fonts(&mut usvg_options);
    let mut renderers = BatchRenderers::new(backend);
    let mut failures = Vec::new();

    for input in svgs {
        println!("[svg] {}", input.display());
        match load_scene(&input, &mut usvg_options) {
            Ok(scene) => {
                if let Err(err) = renderers.render_cpu(&scene, &input) {
                    failures.push(format!("[cpu] {}: {err}", input.display()));
                }
                if let Err(err) = renderers.render_cubecl(&scene, &input) {
                    failures.push(format!("[cubecl] {}: {err}", input.display()));
                }
            }
            Err(err) => failures.push(format!("[scene] {}: {err}", input.display())),
        }
    }

    if !failures.is_empty() {
        eprintln!();
        eprintln!("Failures:");
        for failure in &failures {
            eprintln!("  {failure}");
        }
        return Err(format!("{} SVG render failures", failures.len()).into());
    }

    Ok(())
}

fn load_scene(
    input: &Path,
    options: &mut usvg::Options<'_>,
) -> Result<Scene, Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    options.resources_dir = input.parent().map(Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, options)?;
    let size = tree.size();
    let width = size.width().ceil() as u32;
    let height = size.height().ceil() as u32;
    let mut scene = Scene::new(width, height);
    scene.push_svg(&tree)?;
    Ok(scene)
}

fn collect_svg_files(
    dir: &Path,
    svgs: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_svg_files(&path, svgs)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
        {
            svgs.push(path);
        }
    }
    Ok(())
}

fn output_path(input: &Path, backend: &str) -> PathBuf {
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
