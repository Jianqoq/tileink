use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
};

use peniko::{
    Color,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};
use tileink::{CpuRenderer, FillRule, Image, Radius, Region, Scene};

pub fn example_output(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("cpu_out")
        .join(format!("{name}.png"))
}

pub fn save_image(image: &Image, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut png_writer = encoder.write_header()?;
    let mut rgba = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_le_bytes());
    }
    png_writer.write_image_data(&rgba)?;

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
    save_image(renderer.image(), &out)?;
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
    scene.push_stroke(
        rect_path(rect, radius),
        stroke,
        color,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
}

pub fn fill_circle(scene: &mut Scene, circle: Circle, brush: impl Into<tileink::Brush>) {
    scene.push_path(
        circle.to_path(0.1),
        brush,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
}

pub fn stroke_circle(scene: &mut Scene, circle: Circle, stroke: Stroke, color: Color) {
    scene.push_stroke(
        circle,
        stroke,
        color,
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
}

pub fn canvas_region(width: u32, height: u32) -> Region {
    Region::rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::all(0.0),
    )
}
