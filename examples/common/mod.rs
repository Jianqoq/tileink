// This helper module is compiled into each example binary; every example uses
// a different subset of the shared scene/render utilities.
#![allow(dead_code)]

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use peniko::{
    Color,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};
use tileink::{Canvas, CpuRenderer, FillRule, Image, Radius, Region, SvgOptions, WgpuRenderer};

pub const EXAMPLE_WIDTH: u32 = 1920;
pub const EXAMPLE_HEIGHT: u32 = 1080;

thread_local! {
    static CPU_RENDERER: RefCell<Option<CpuRenderer>> = const { RefCell::new(None) };
    static WGPU_RENDERERS: RefCell<HashMap<(WgpuMode, u32, u32), WgpuRenderer>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum WgpuMode {
    Native,
    Portable,
}

pub fn example_output(name: &str) -> PathBuf {
    backend_output("cpu", name)
}

pub fn wgpu_example_output(name: &str) -> PathBuf {
    backend_output(wgpu_backend_name(), name)
}

fn backend_output(backend: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(backend)
        .join("out")
        .join(format!("{name}.png"))
}

pub fn example_asset(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
}

pub fn load_svg_scene(
    input: impl AsRef<Path>,
    target_width: u32,
) -> Result<(Canvas, u32, u32), Box<dyn std::error::Error>> {
    let input = input.as_ref();
    let data = fs::read(input)?;
    let mut options = usvg::Options {
        resources_dir: input.parent().map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    load_svg_fonts(&mut options);

    let tree = usvg::Tree::from_data(&data, &options)?;
    let size = tree
        .size()
        .to_int_size()
        .scale_to_width(target_width)
        .ok_or("SVG size must be positive")?;
    let width = size.width();
    let height = size.height();
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
    Ok((scene, width, height))
}

fn load_svg_fonts(options: &mut usvg::Options<'_>) {
    let fonts_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("svg")
        .join("fonts");
    options.fontdb_mut().load_fonts_dir(fonts_dir);
}

pub fn save_image(image: &Image, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    image.save(path)?;
    Ok(())
}

pub fn save_example_image(
    image: &Image,
    path: impl AsRef<Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    save_image(image, path)
}

pub fn render_to_png(
    name: &str,
    scene: &Canvas,
    width: u32,
    height: u32,
    clear: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let out = example_output(name);
    CPU_RENDERER.with(|renderer| -> Result<(), Box<dyn std::error::Error>> {
        let mut renderer = renderer.borrow_mut();
        let renderer = renderer.get_or_insert_with(|| CpuRenderer::new(width, height, clear));
        renderer.set_clear_color(clear);
        renderer.render(scene);
        save_example_image(renderer.image(), &out)
    })?;
    println!("Wrote {}", out.display());
    Ok(())
}

pub fn render_to_png_wgpu(
    name: &str,
    scene: &Canvas,
    width: u32,
    height: u32,
    clear: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    render_to_png_wgpu_with(name, width, height, clear, |renderer| {
        renderer.render(scene);
        Ok(())
    })
}

pub fn render_to_png_wgpu_with(
    name: &str,
    width: u32,
    height: u32,
    clear: Color,
    mut render: impl FnMut(&mut WgpuRenderer) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if wgpu_compare_portable_mode() {
        return render_to_png_wgpu_compare_portable(name, width, height, clear, render);
    }

    let out = wgpu_example_output(name);
    WGPU_RENDERERS.with(|renderers| -> Result<(), Box<dyn std::error::Error>> {
        let mut renderers = renderers.borrow_mut();
        let renderer = renderers
            .entry((wgpu_mode(), width, height))
            .or_insert_with(|| new_wgpu_renderer_for_mode(width, height, clear, wgpu_mode()));
        renderer.set_clear_color(clear);
        render(renderer)?;
        save_example_image(&renderer.image(), &out)
    })?;
    println!("Wrote {}", out.display());
    Ok(())
}

pub fn new_wgpu_renderer(width: u32, height: u32, clear: Color) -> WgpuRenderer {
    new_wgpu_renderer_for_mode(width, height, clear, wgpu_mode())
}

fn render_to_png_wgpu_compare_portable(
    name: &str,
    width: u32,
    height: u32,
    clear: Color,
    mut render: impl FnMut(&mut WgpuRenderer) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Native is the example artifact; portable renders in memory to catch WebGPU regressions
    // without producing a second set of PNGs.
    let native_out = backend_output("wgpu", name);
    WGPU_RENDERERS.with(|renderers| -> Result<(), Box<dyn std::error::Error>> {
        let mut renderers = renderers.borrow_mut();
        let native = renderers
            .entry((WgpuMode::Native, width, height))
            .or_insert_with(|| new_wgpu_renderer_for_mode(width, height, clear, WgpuMode::Native));
        native.set_clear_color(clear);
        render(native)?;
        let native_image = native.image();
        save_example_image(&native_image, &native_out)?;
        let native_image = native_image.clone();
        println!("Wrote {}", native_out.display());

        let portable = renderers
            .entry((WgpuMode::Portable, width, height))
            .or_insert_with(|| {
                new_wgpu_renderer_for_mode(width, height, clear, WgpuMode::Portable)
            });
        portable.set_clear_color(clear);
        render(portable)?;
        let portable_image = portable.image();
        assert_images_equal(name, &native_image, &portable_image)?;
        println!("[wgpu-portable] matched {}", native_out.display());
        Ok(())
    })
}

fn new_wgpu_renderer_for_mode(
    width: u32,
    height: u32,
    clear: Color,
    mode: WgpuMode,
) -> WgpuRenderer {
    if mode == WgpuMode::Native {
        return WgpuRenderer::new_default_device(width, height, clear);
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("request portable wgpu adapter");
    let required_features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tileink portable example device"),
        required_features,
        required_limits: adapter.limits(),
        memory_hints: wgpu::MemoryHints::MemoryUsage,
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("request portable wgpu device");
    WgpuRenderer::new(&device, &queue, width, height, clear)
}

pub fn wgpu_backend_name() -> &'static str {
    match wgpu_mode() {
        WgpuMode::Native => "wgpu",
        WgpuMode::Portable => "wgpu_portable",
    }
}

fn wgpu_mode() -> WgpuMode {
    if matches!(
        std::env::var("TILEINK_WGPU_MODE").as_deref(),
        Ok("portable")
    ) || matches!(
        std::env::var("TILEINK_WGPU_PORTABLE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    ) {
        WgpuMode::Portable
    } else {
        WgpuMode::Native
    }
}

fn wgpu_compare_portable_mode() -> bool {
    matches!(
        std::env::var("TILEINK_WGPU_COMPARE_PORTABLE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

fn assert_images_equal(
    name: &str,
    native: &Image,
    portable: &Image,
) -> Result<(), Box<dyn std::error::Error>> {
    if native.width != portable.width || native.height != portable.height {
        return Err(format!(
            "{name}: WGPU native size {}x{} differs from portable size {}x{}",
            native.width, native.height, portable.width, portable.height
        )
        .into());
    }
    if native.pixels == portable.pixels {
        return Ok(());
    }

    let mut first = None;
    let mut diff_count = 0usize;
    for (ix, (native_px, portable_px)) in native.pixels.iter().zip(&portable.pixels).enumerate() {
        if native_px == portable_px {
            continue;
        }
        diff_count += 1;
        if first.is_none() {
            first = Some((ix, native_px.to_le_bytes(), portable_px.to_le_bytes()));
        }
    }
    let (ix, native_px, portable_px) = first.expect("diff count is non-zero");
    Err(format!(
        "{name}: WGPU portable differs from native at ({}, {}), native={:?}, portable={:?}, differing_pixels={}",
        ix as u32 % native.width,
        ix as u32 / native.width,
        native_px,
        portable_px,
        diff_count
    )
    .into())
}

pub fn rect_path(rect: Rect, radius: Radius) -> BezPath {
    if radius.top_left == 0.0
        && radius.top_right == 0.0
        && radius.bottom_left == 0.0
        && radius.bottom_right == 0.0
    {
        return rect.to_path(0.0);
    }
    peniko::kurbo::RoundedRect::new(
        rect.x0,
        rect.y0,
        rect.x1,
        rect.y1,
        (
            radius.top_left as f64,
            radius.top_right as f64,
            radius.bottom_right as f64,
            radius.bottom_left as f64,
        ),
    )
    .to_path(0.1)
}

pub fn fill_rect(scene: &mut Canvas, rect: Rect, radius: Radius, brush: impl Into<tileink::Brush>) {
    scene.push_path(
        rect_path(rect, radius),
        brush,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
}

pub fn stroke_rect(scene: &mut Canvas, rect: Rect, radius: Radius, stroke: Stroke, color: Color) {
    scene.push_rect_stroke(rect, radius, stroke, color);
}

pub fn fill_circle(scene: &mut Canvas, circle: Circle, brush: impl Into<tileink::Brush>) {
    scene.push_circle(circle, brush);
}

pub fn stroke_circle(scene: &mut Canvas, circle: Circle, stroke: Stroke, color: Color) {
    scene.push_circle_stroke(circle, stroke, color);
}

pub fn canvas_region(width: u32, height: u32) -> Region {
    Region::rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
    )
}
