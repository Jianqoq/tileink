#[path = "common/mod.rs"]
mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use peniko::{Color, kurbo::Affine};
use tileink::{CpuRenderer, Scene, SvgOptions, WgpuRenderer};

const REFERENCE_IMAGE_WIDTH: u32 = 300;

#[derive(Clone, Copy)]
enum Backend {
    Both,
    Cpu,
    Wgpu,
}

impl Backend {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "both" => Ok(Self::Both),
            "cpu" => Ok(Self::Cpu),
            "wgpu" => Ok(Self::Wgpu),
            _ => Err(format!(
                "unknown backend `{value}`, expected both, cpu, or wgpu"
            )),
        }
    }

    fn renders_cpu(self) -> bool {
        matches!(self, Self::Both | Self::Cpu)
    }

    fn renders_wgpu(self) -> bool {
        matches!(self, Self::Both | Self::Wgpu)
    }
}

struct BatchRenderers {
    cpu: Option<CpuRenderer>,
    wgpu: Option<WgpuRenderer>,
}

impl BatchRenderers {
    fn new(backend: Backend) -> Self {
        Self {
            cpu: backend
                .renders_cpu()
                .then(|| CpuRenderer::new(1, 1, Color::TRANSPARENT)),
            wgpu: backend
                .renders_wgpu()
                .then(|| WgpuRenderer::new_default_device(1, 1, Color::TRANSPARENT)),
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

    fn render_wgpu(
        &mut self,
        scene: &Scene,
        input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(renderer) = &mut self.wgpu else {
            return Ok(());
        };
        let output = output_path(input, "wgpu");
        renderer.render(scene);
        let image = renderer.image();
        common::save_image(&image, &output)?;
        println!("[wgpu] wrote {}", output.display());
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (root, backend) = parse_args()?;

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
                if let Err(err) = renderers.render_wgpu(&scene, &input) {
                    failures.push(format!("[wgpu] {}: {err}", input.display()));
                }
            }
            Err(err) => {
                failures.push(format!("[scene] {}: {err}", input.display()));
            }
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

fn parse_args() -> Result<(PathBuf, Backend), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err("usage: svg_fixture_render <folder> [both|cpu|wgpu]".into());
    }

    let root = PathBuf::from(&args[0]);
    let mut backend = Backend::Both;
    let mut ix = 1;

    if args.get(ix).is_some_and(|value| !value.starts_with("--")) {
        backend = Backend::parse(&args[ix])?;
        ix += 1;
    }

    if ix < args.len() {
        return Err(format!("unknown argument `{}`", args[ix]).into());
    }

    Ok((root, backend))
}

fn load_scene(
    input: &Path,
    options: &mut usvg::Options<'_>,
) -> Result<Scene, Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    options.resources_dir = input.parent().map(Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, options)?;
    let size = scaled_reference_size(tree.size(), REFERENCE_IMAGE_WIDTH)?;
    let width = size.0;
    let height = size.1;
    let scale_x = width as f64 / tree.size().width() as f64;
    let scale_y = height as f64 / tree.size().height() as f64;
    let mut scene = Scene::new(width, height);
    scene.push_svg_with_options(
        &tree,
        SvgOptions {
            transform: Affine::scale_non_uniform(scale_x, scale_y),
            ..SvgOptions::default()
        },
    )?;
    Ok(scene)
}

fn scaled_reference_size(
    size: usvg::Size,
    target_width: u32,
) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    let size = size
        .to_int_size()
        .scale_to_width(target_width)
        .ok_or("SVG size must be positive")?;
    Ok((size.width(), size.height()))
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
