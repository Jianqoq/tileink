// This helper module is compiled into each example binary; every example uses
// a different subset of the shared scene/render utilities.
#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use peniko::{
    Color,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};
use tileink::{CpuRenderer, CubeWgpuRenderer, FillRule, Image, Radius, Region, Scene, SvgOptions};

pub const EXAMPLE_WIDTH: u32 = 1920;
pub const EXAMPLE_HEIGHT: u32 = 1080;

pub fn example_output(name: &str) -> PathBuf {
    backend_output("cpu", name)
}

pub fn cubecl_example_output(name: &str) -> PathBuf {
    backend_output("cubecl", name)
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
) -> Result<(Scene, u32, u32), Box<dyn std::error::Error>> {
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
    let mut scene = Scene::new(width, height);
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
    if image.width == EXAMPLE_WIDTH && image.height == EXAMPLE_HEIGHT {
        return save_image(image, path);
    }
    fit_image_to_example_size(image).save(path)?;
    Ok(())
}

pub fn render_to_png(
    name: &str,
    scene: &Scene,
    width: u32,
    height: u32,
    clear: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = CpuRenderer::new(width, height, clear);
    renderer.render(scene);
    let out = example_output(name);
    save_example_image(renderer.image(), &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}

pub fn render_to_png_cubecl(
    name: &str,
    scene: &Scene,
    width: u32,
    height: u32,
    clear: Color,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, clear);
    renderer.render(scene);
    let image = renderer.image();
    let out = cubecl_example_output(name);
    save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}

fn fit_image_to_example_size(image: &Image) -> Image {
    if image.width == 0 || image.height == 0 {
        return Image::new(EXAMPLE_WIDTH, EXAMPLE_HEIGHT, Color::TRANSPARENT);
    }

    let scale = (EXAMPLE_WIDTH as f32 / image.width as f32)
        .min(EXAMPLE_HEIGHT as f32 / image.height as f32);
    let scaled_width = ((image.width as f32 * scale).round() as u32).clamp(1, EXAMPLE_WIDTH);
    let scaled_height = ((image.height as f32 * scale).round() as u32).clamp(1, EXAMPLE_HEIGHT);
    let offset_x = (EXAMPLE_WIDTH - scaled_width) / 2;
    let offset_y = (EXAMPLE_HEIGHT - scaled_height) / 2;
    let fill = image.pixels.first().copied().unwrap_or(0);
    let mut output = Image {
        width: EXAMPLE_WIDTH,
        height: EXAMPLE_HEIGHT,
        pixels: vec![fill; (EXAMPLE_WIDTH * EXAMPLE_HEIGHT) as usize],
    };

    for y in 0..scaled_height {
        let sy = ((y as f32 + 0.5) / scale - 0.5).clamp(0.0, image.height as f32 - 1.0);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(image.height - 1);
        let ty = sy - y0 as f32;
        for x in 0..scaled_width {
            let sx = ((x as f32 + 0.5) / scale - 0.5).clamp(0.0, image.width as f32 - 1.0);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(image.width - 1);
            let tx = sx - x0 as f32;
            let pixel = bilinear_pixel(
                image.pixels[(y0 * image.width + x0) as usize],
                image.pixels[(y0 * image.width + x1) as usize],
                image.pixels[(y1 * image.width + x0) as usize],
                image.pixels[(y1 * image.width + x1) as usize],
                tx,
                ty,
            );
            output.pixels[((offset_y + y) * EXAMPLE_WIDTH + offset_x + x) as usize] = pixel;
        }
    }

    output
}

fn bilinear_pixel(p00: u32, p10: u32, p01: u32, p11: u32, tx: f32, ty: f32) -> u32 {
    let [r00, g00, b00, a00] = p00.to_le_bytes();
    let [r10, g10, b10, a10] = p10.to_le_bytes();
    let [r01, g01, b01, a01] = p01.to_le_bytes();
    let [r11, g11, b11, a11] = p11.to_le_bytes();
    u32::from_le_bytes([
        bilinear_channel(r00, r10, r01, r11, tx, ty),
        bilinear_channel(g00, g10, g01, g11, tx, ty),
        bilinear_channel(b00, b10, b01, b11, tx, ty),
        bilinear_channel(a00, a10, a01, a11, tx, ty),
    ])
}

fn bilinear_channel(c00: u8, c10: u8, c01: u8, c11: u8, tx: f32, ty: f32) -> u8 {
    let top = c00 as f32 + (c10 as f32 - c00 as f32) * tx;
    let bottom = c01 as f32 + (c11 as f32 - c01 as f32) * tx;
    (top + (bottom - top) * ty + 0.5).clamp(0.0, 255.0) as u8
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

pub fn fill_rect(scene: &mut Scene, rect: Rect, radius: Radius, brush: impl Into<tileink::Brush>) {
    scene.push_path(
        rect_path(rect, radius),
        brush,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
}

pub fn stroke_rect(scene: &mut Scene, rect: Rect, radius: Radius, stroke: Stroke, color: Color) {
    scene.push_rect_stroke(rect, radius, stroke, color, FillRule::NonZero);
}

pub fn fill_circle(scene: &mut Scene, circle: Circle, brush: impl Into<tileink::Brush>) {
    scene.push_circle(circle, brush, FillRule::NonZero);
}

pub fn stroke_circle(scene: &mut Scene, circle: Circle, stroke: Stroke, color: Color) {
    scene.push_circle_stroke(circle, stroke, color, FillRule::NonZero);
}

pub fn canvas_region(width: u32, height: u32) -> Region {
    Region::rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
    )
}
