// This helper module is compiled into each example binary; every example uses
// a different subset of the shared scene/render utilities.
#![allow(dead_code)]

pub mod fonts;
pub mod liquid_glass_fast_path;

use std::{
    fs,
    path::{Path, PathBuf},
};

use peniko::{
    Color,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};
use tileink::{Canvas, FillRule, Image, Radius, Region, SvgOptions};

pub const EXAMPLE_WIDTH: u32 = 1920;
pub const EXAMPLE_HEIGHT: u32 = 1080;

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
    let mut options = svg_options();
    options.resources_dir = input.parent().map(Path::to_path_buf);
    let tree = usvg::Tree::from_data(&data, &options)?;
    svg_tree_to_scene(&tree, target_width)
}

pub fn svg_options() -> usvg::Options<'static> {
    let mut options = usvg::Options::default();
    load_svg_fonts(&mut options);
    // SVGs without a usable font-family still need a face from the bundled font database.
    options.font_family = "Noto Serif".to_owned();
    options
}

pub fn svg_tree_to_scene(
    tree: &usvg::Tree,
    target_width: u32,
) -> Result<(Canvas, u32, u32), Box<dyn std::error::Error>> {
    let size = tree
        .size()
        .to_int_size()
        .scale_to_width(target_width)
        .ok_or("SVG size must be positive")?;
    let width = size.width();
    let height = size.height();
    let scale_x = width as f64 / tree.size().width() as f64;
    let scale_y = height as f64 / tree.size().height() as f64;
    let mut scene = Canvas::new(width, height, 1.0);
    scene.push_svg_with_options(
        tree,
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
    let fontdb = options.fontdb_mut();
    fontdb.load_fonts_dir(fonts_dir);
    // Resolve generic SVG families to bundled faces; platform defaults may not be installed.
    fontdb.set_serif_family("Noto Serif");
    fontdb.set_sans_serif_family("Noto Sans");
    fontdb.set_monospace_family("Noto Mono");
    fontdb.set_cursive_family("Yellowtail");
    fontdb.set_fantasy_family("Sedgwick Ave Display");
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

/// Use immutable captured font inputs when running explicit backend references.
pub fn new_font_system() -> tileink::TextFontSystem {
    tileink::TextFontSystem::new()
}
