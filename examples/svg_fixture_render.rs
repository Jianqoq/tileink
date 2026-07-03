#[path = "common/mod.rs"]
mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use peniko::{Color, kurbo::Affine};
use tileink::{CpuRenderer, CubeWgpuRenderer, Scene, SvgOptions};
#[cfg(feature = "wgpu")]
use tileink::{Image, TextContext, WgpuRenderer};

const REFERENCE_IMAGE_WIDTH: u32 = 300;

#[derive(Clone, Copy)]
enum Backend {
    Both,
    Cpu,
    CubeCl,
    #[cfg(feature = "wgpu")]
    Wgpu,
    #[cfg(feature = "wgpu")]
    CubeClWgpu,
}

impl Backend {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "both" => Ok(Self::Both),
            "cpu" => Ok(Self::Cpu),
            "cubecl" => Ok(Self::CubeCl),
            #[cfg(feature = "wgpu")]
            "wgpu" => Ok(Self::Wgpu),
            #[cfg(not(feature = "wgpu"))]
            "wgpu" => Err("backend `wgpu` requires building with `--features wgpu`".to_string()),
            #[cfg(feature = "wgpu")]
            "cubecl-wgpu" => Ok(Self::CubeClWgpu),
            #[cfg(not(feature = "wgpu"))]
            "cubecl-wgpu" => {
                Err("backend `cubecl-wgpu` requires building with `--features wgpu`".to_string())
            }
            _ => Err(format!(
                "unknown backend `{value}`, expected both, cpu, cubecl, wgpu, or cubecl-wgpu"
            )),
        }
    }

    fn renders_cpu(self) -> bool {
        matches!(self, Self::Both | Self::Cpu)
    }

    fn renders_cubecl(self) -> bool {
        match self {
            Self::Both | Self::CubeCl => true,
            #[cfg(feature = "wgpu")]
            Self::CubeClWgpu => true,
            _ => false,
        }
    }

    #[cfg(feature = "wgpu")]
    fn renders_wgpu(self) -> bool {
        matches!(self, Self::Wgpu | Self::CubeClWgpu)
    }

    #[cfg(feature = "wgpu")]
    fn compares_cubecl_wgpu(self) -> bool {
        matches!(self, Self::CubeClWgpu)
    }

    #[cfg(not(feature = "wgpu"))]
    fn compares_cubecl_wgpu(self) -> bool {
        false
    }
}

struct BatchRenderers {
    cpu: Option<CpuRenderer>,
    cubecl: Option<CubeWgpuRenderer>,
    #[cfg(feature = "wgpu")]
    text_context: TextContext,
    #[cfg(feature = "wgpu")]
    wgpu: Option<WgpuRenderer>,
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
            #[cfg(feature = "wgpu")]
            text_context: TextContext::new(),
            #[cfg(feature = "wgpu")]
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
        let image = renderer.image();
        common::save_image(&image, &output)?;
        println!("[cubecl] wrote {}", output.display());
        Ok(())
    }

    #[cfg(feature = "wgpu")]
    fn render_wgpu(
        &mut self,
        scene: &Scene,
        input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(renderer) = &mut self.wgpu else {
            return Ok(());
        };
        let output = output_path(input, "wgpu");
        if !renderer.render_native(scene) {
            return Err(
                format!("native wgpu renderer does not support {}", input.display()).into(),
            );
        }
        let image = renderer.image();
        common::save_image(&image, &output)?;
        println!("[wgpu] wrote {}", output.display());
        Ok(())
    }

    #[cfg(not(feature = "wgpu"))]
    fn render_wgpu(
        &mut self,
        _scene: &Scene,
        _input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    #[cfg(feature = "wgpu")]
    fn compare_cubecl_wgpu(
        &mut self,
        scene: &Scene,
        input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(cubecl) = &mut self.cubecl else {
            return Ok(());
        };
        let Some(wgpu) = &mut self.wgpu else {
            return Ok(());
        };

        cubecl.render_with_text(scene, &mut self.text_context);
        let cubecl_image = cubecl.image();
        if !wgpu.render_native_with_text(scene, &mut self.text_context) {
            return Err(
                format!("native wgpu renderer does not support {}", input.display()).into(),
            );
        }
        let wgpu_image = wgpu.image();
        assert_images_equal(&cubecl_image, &wgpu_image, input)?;
        println!("[cubecl-wgpu] exact match {}", input.display());
        Ok(())
    }

    #[cfg(not(feature = "wgpu"))]
    fn compare_cubecl_wgpu(
        &mut self,
        _scene: &Scene,
        _input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
                if backend.compares_cubecl_wgpu() {
                    if let Err(err) = renderers.compare_cubecl_wgpu(&scene, &input) {
                        failures.push(format!("[cubecl-wgpu] {}: {err}", input.display()));
                    }
                } else {
                    if let Err(err) = renderers.render_cpu(&scene, &input) {
                        failures.push(format!("[cpu] {}: {err}", input.display()));
                    }
                    if let Err(err) = renderers.render_cubecl(&scene, &input) {
                        failures.push(format!("[cubecl] {}: {err}", input.display()));
                    }
                    if let Err(err) = renderers.render_wgpu(&scene, &input) {
                        failures.push(format!("[wgpu] {}: {err}", input.display()));
                    }
                }
            }
            Err(err) => {
                if backend.compares_cubecl_wgpu() {
                    println!("[scene] skipped {}: {err}", input.display());
                } else {
                    failures.push(format!("[scene] {}: {err}", input.display()));
                }
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
        return Err("usage: svg_fixture_render <folder> [both|cpu|cubecl|wgpu|cubecl-wgpu]".into());
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

#[cfg(feature = "wgpu")]
fn assert_images_equal(
    cubecl: &Image,
    wgpu: &Image,
    input: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if (cubecl.width, cubecl.height) != (wgpu.width, wgpu.height) {
        return Err(format!(
            "{} dimensions differ: cubecl {}x{}, wgpu {}x{}",
            input.display(),
            cubecl.width,
            cubecl.height,
            wgpu.width,
            wgpu.height
        )
        .into());
    }

    let mut mismatch_count = 0usize;
    let mut first_mismatch = None;
    for (ix, (&cubecl_px, &wgpu_px)) in cubecl.pixels.iter().zip(&wgpu.pixels).enumerate() {
        if cubecl_px != wgpu_px {
            mismatch_count += 1;
            if first_mismatch.is_none() {
                let x = ix as u32 % cubecl.width;
                let y = ix as u32 / cubecl.width;
                first_mismatch = Some((x, y, cubecl.rgba8_at(x, y), wgpu.rgba8_at(x, y)));
            }
        }
    }

    if let Some((x, y, cubecl_rgba, wgpu_rgba)) = first_mismatch {
        return Err(format!(
            "{} has {mismatch_count} differing pixels; first at ({x}, {y}): cubecl {cubecl_rgba:?}, wgpu {wgpu_rgba:?}",
            input.display()
        )
        .into());
    }
    Ok(())
}

fn load_svg_test_fonts(options: &mut usvg::Options<'_>) {
    let fonts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("svg")
        .join("fonts");
    options.fontdb_mut().load_fonts_dir(fonts_dir);
}
