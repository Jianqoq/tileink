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
    static WGPU_RENDERERS: RefCell<HashMap<(u32, u32), WgpuRenderer>> = RefCell::new(HashMap::new());
}

pub fn example_output(name: &str) -> PathBuf {
    backend_output("cpu", name)
}

pub fn wgpu_example_output(name: &str) -> PathBuf {
    backend_output("wgpu", name)
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
    let out = wgpu_example_output(name);
    WGPU_RENDERERS.with(|renderers| -> Result<(), Box<dyn std::error::Error>> {
        let mut renderers = renderers.borrow_mut();
        let renderer = renderers
            .entry((width, height))
            .or_insert_with(|| WgpuRenderer::new_default_device(width, height, clear));
        renderer.set_clear_color(clear);
        renderer.render(scene);
        save_example_image(&renderer.image(), &out)
    })?;
    println!("Wrote {}", out.display());
    Ok(())
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
