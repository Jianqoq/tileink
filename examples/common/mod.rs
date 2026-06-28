use std::path::{Path, PathBuf};

use peniko::{
    Color,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};
use tileink::{CpuRenderer, CubeWgpuRenderer, FillRule, Image, Radius, Region, Scene};

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

pub fn save_image(image: &Image, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    image.save(path)?;
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
    save_image(&image, &out)?;
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
        Radius::all(0.0),
    )
}
