#[path = "common/mod.rs"]
mod common;

use std::{
    fs,
    path::{Path, PathBuf},
};

use peniko::{Color, kurbo::Affine};
use tileink::{Canvas, CpuRenderer, Image, SvgOptions, WgpuRenderer};

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

#[derive(Clone, Copy)]
enum WgpuMode {
    Native,
    Portable,
}

impl WgpuMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "native" => Ok(Self::Native),
            "portable" => Ok(Self::Portable),
            _ => Err(format!(
                "unknown wgpu mode `{value}`, expected native or portable"
            )),
        }
    }

    fn output_suffix(self) -> &'static str {
        match self {
            Self::Native => "wgpu",
            Self::Portable => "wgpu-portable",
        }
    }
}

struct BatchRenderers {
    cpu: Option<CpuRenderer>,
    wgpu: Option<WgpuRenderer>,
    wgpu_portable: Option<WgpuRenderer>,
    wgpu_mode: WgpuMode,
    compare_wgpu_portable: bool,
}

impl BatchRenderers {
    fn new(backend: Backend, wgpu_mode: WgpuMode, compare_wgpu_portable: bool) -> Self {
        Self {
            cpu: backend
                .renders_cpu()
                .then(|| CpuRenderer::new(1, 1, Color::TRANSPARENT)),
            wgpu: backend.renders_wgpu().then(|| {
                new_wgpu_renderer_for_mode(
                    1,
                    1,
                    Color::TRANSPARENT,
                    if compare_wgpu_portable {
                        WgpuMode::Native
                    } else {
                        wgpu_mode
                    },
                )
            }),
            wgpu_portable: (backend.renders_wgpu() && compare_wgpu_portable)
                .then(|| new_wgpu_renderer_for_mode(1, 1, Color::TRANSPARENT, WgpuMode::Portable)),
            wgpu_mode,
            compare_wgpu_portable,
        }
    }

    fn render_cpu(
        &mut self,
        scene: &Canvas,
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
        scene: &Canvas,
        input: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(renderer) = &mut self.wgpu else {
            return Ok(());
        };
        if self.compare_wgpu_portable {
            let output = output_path(input, WgpuMode::Native.output_suffix());
            renderer.render(scene);
            let native = renderer.image();
            common::save_image(&native, &output)?;
            println!("[wgpu] wrote {}", output.display());

            let portable = self
                .wgpu_portable
                .as_mut()
                .ok_or("portable WGPU renderer was not initialized")?;
            portable.render(scene);
            let portable = portable.image();
            assert_images_equal(input, &native, &portable)?;
            println!("[wgpu-portable] matched {}", output.display());
            return Ok(());
        }

        let suffix = self.wgpu_mode.output_suffix();
        let output = output_path(input, suffix);
        renderer.render(scene);
        let image = renderer.image();
        common::save_image(&image, &output)?;
        println!("[{suffix}] wrote {}", output.display());
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (root, backend, wgpu_mode, compare_wgpu_portable) = parse_args()?;

    if !root.is_dir() {
        return Err(format!("input must be a folder: {}", root.display()).into());
    }

    let mut svgs = Vec::new();
    collect_svg_files(&root, &mut svgs)?;
    svgs.sort();

    let mut usvg_options = usvg::Options::default();
    load_svg_test_fonts(&mut usvg_options);
    let mut renderers = BatchRenderers::new(backend, wgpu_mode, compare_wgpu_portable);
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

fn parse_args() -> Result<(PathBuf, Backend, WgpuMode, bool), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return Err(
            "usage: svg_fixture_render <folder> [both|cpu|wgpu] [--wgpu-mode native|portable] [--compare-wgpu-portable]"
                .into(),
        );
    }

    let root = PathBuf::from(&args[0]);
    let mut backend = Backend::Both;
    let mut wgpu_mode = WgpuMode::Native;
    let mut compare_wgpu_portable = false;
    let mut ix = 1;

    if args.get(ix).is_some_and(|value| !value.starts_with("--")) {
        backend = Backend::parse(&args[ix])?;
        ix += 1;
    }

    while ix < args.len() {
        match args[ix].as_str() {
            "--wgpu-mode" => {
                ix += 1;
                let value = args.get(ix).ok_or("--wgpu-mode requires a value")?;
                wgpu_mode = WgpuMode::parse(value)?;
            }
            "--compare-wgpu-portable" => {
                compare_wgpu_portable = true;
            }
            arg => return Err(format!("unknown argument `{arg}`").into()),
        }
        ix += 1;
    }

    Ok((root, backend, wgpu_mode, compare_wgpu_portable))
}

fn new_wgpu_renderer_for_mode(
    width: u32,
    height: u32,
    clear: Color,
    mode: WgpuMode,
) -> WgpuRenderer {
    if matches!(mode, WgpuMode::Native) {
        return WgpuRenderer::new_default_device(width, height, clear);
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("request portable wgpu adapter");
    let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink portable svg fixture device"),
        required_features,
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("request portable wgpu device");
    WgpuRenderer::new(&device, &queue, width, height, clear)
}

fn load_scene(
    input: &Path,
    options: &mut usvg::Options<'_>,
) -> Result<Canvas, Box<dyn std::error::Error>> {
    let data = fs::read(input)?;
    options.resources_dir = input.parent().map(Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, options)?;
    let size = scaled_reference_size(tree.size(), REFERENCE_IMAGE_WIDTH)?;
    let width = size.0;
    let height = size.1;
    let scale_x = width as f64 / tree.size().width() as f64;
    let scale_y = height as f64 / tree.size().height() as f64;
    let mut scene = Canvas::new(width, height);
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

fn assert_images_equal(
    input: &Path,
    native: &Image,
    portable: &Image,
) -> Result<(), Box<dyn std::error::Error>> {
    if native.width != portable.width || native.height != portable.height {
        return Err(format!(
            "{}: WGPU native size {}x{} differs from portable size {}x{}",
            input.display(),
            native.width,
            native.height,
            portable.width,
            portable.height
        )
        .into());
    }
    if native.pixels == portable.pixels {
        return Ok(());
    }

    let mut first = None;
    let mut differing_pixels = 0usize;
    for (ix, (native_px, portable_px)) in native.pixels.iter().zip(&portable.pixels).enumerate() {
        if native_px == portable_px {
            continue;
        }
        differing_pixels += 1;
        first.get_or_insert((
            ix as u32,
            native_px.to_le_bytes(),
            portable_px.to_le_bytes(),
        ));
    }
    let (ix, native_px, portable_px) = first.unwrap();
    Err(format!(
        "{}: WGPU portable differs from native at ({}, {}), native={:?}, portable={:?}, differing_pixels={}",
        input.display(),
        ix % native.width,
        ix / native.width,
        native_px,
        portable_px,
        differing_pixels
    )
    .into())
}

fn load_svg_test_fonts(options: &mut usvg::Options<'_>) {
    let fonts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("svg")
        .join("fonts");
    options.fontdb_mut().load_fonts_dir(fonts_dir);
}
