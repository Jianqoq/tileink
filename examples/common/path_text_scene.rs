use peniko::{
    Color, Extend, Gradient,
    color::palette::css,
    kurbo::{Affine, Point, Rect},
};
use tileink::{
    FillRule, Scene, TextAttrs, TextCacheKeyFlags, TextContext, TextLayoutOptions, TextWeight,
};

const DESIGN_WIDTH: u32 = 780;
const DESIGN_HEIGHT: u32 = 400;

pub const WIDTH: u32 = crate::common::EXAMPLE_WIDTH;
pub const HEIGHT: u32 = crate::common::EXAMPLE_HEIGHT;
pub const CLEAR: Color = Color::from_rgb8(248, 250, 252);

fn offset() -> (f64, f64) {
    (
        (WIDTH as f64 - DESIGN_WIDTH as f64) * 0.5,
        (HEIGHT as f64 - DESIGN_HEIGHT as f64) * 0.5,
    )
}

fn point(x: f64, y: f64) -> Point {
    let (dx, dy) = offset();
    Point::new(x + dx, y + dy)
}

fn point_tuple(x: f64, y: f64) -> (f64, f64) {
    let p = point(x, y);
    (p.x, p.y)
}

pub fn scene(context: &mut TextContext) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.push_rect(
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        tileink::Radius::ZERO,
        CLEAR,
        FillRule::NonZero,
    );

    push_bitmap(
        &mut scene,
        context,
        "Bitmap text: swash raster, hinted, subpixel",
        16.0,
        point(28.0, 36.0),
        Color::from_rgb8(15, 23, 42),
        TextAttrs::new().weight(TextWeight::SEMIBOLD),
    );

    push_bitmap(
        &mut scene,
        context,
        "Hamburgefonts 12px",
        12.0,
        point(28.0, 82.0),
        Color::BLACK,
        TextAttrs::new(),
    );
    push_label(&mut scene, context, "bitmap raster", point(220.0, 82.0));

    push_path_text(
        &mut scene,
        context,
        "Hamburgefonts 12px",
        12.0,
        point(28.0, 124.0),
        Color::BLACK,
        TextAttrs::new(),
        Affine::IDENTITY,
    );
    push_label(
        &mut scene,
        context,
        "path outline, hinting on",
        point(220.0, 124.0),
    );

    push_path_text(
        &mut scene,
        context,
        "Hamburgefonts 12px",
        12.0,
        point(28.0, 166.0),
        Color::BLACK,
        TextAttrs::new().cache_key_flags(TextCacheKeyFlags::DISABLE_HINTING),
        Affine::IDENTITY,
    );
    push_label(
        &mut scene,
        context,
        "path outline, hinting off",
        point(220.0, 166.0),
    );

    let gradient = Gradient::new_linear(point_tuple(24.0, 205.0), point_tuple(610.0, 205.0))
        .with_extend(Extend::Pad)
        .with_stops([css::CRIMSON, css::ORANGE, css::DODGER_BLUE]);
    push_path_text(
        &mut scene,
        context,
        "VECTOR PATH",
        62.0,
        point(28.0, 252.0),
        &gradient,
        TextAttrs::new().weight(TextWeight::BOLD),
        Affine::rotate_about(-5.0_f64.to_radians(), point_tuple(330.0, 226.0)),
    );

    push_path_text(
        &mut scene,
        context,
        "Affine transform + gradient brush",
        25.0,
        point(30.0, 362.0),
        Color::from_rgb8(15, 118, 110),
        TextAttrs::new().weight(TextWeight::SEMIBOLD),
        Affine::rotate_about(4.0_f64.to_radians(), point_tuple(240.0, 352.0)),
    );

    scene
}

fn push_label(scene: &mut Scene, context: &mut TextContext, text: &str, origin: Point) {
    push_bitmap(
        scene,
        context,
        text,
        11.0,
        origin,
        Color::from_rgb8(71, 85, 105),
        TextAttrs::new(),
    );
}

fn push_bitmap(
    scene: &mut Scene,
    context: &mut TextContext,
    text: &str,
    font_size: f32,
    origin: Point,
    color: Color,
    attrs: TextAttrs<'_>,
) {
    let layout = context.layout(TextLayoutOptions::new(text, font_size).with_attrs(attrs));
    scene.push_text_layout(&layout, origin, color);
}

fn push_path_text(
    scene: &mut Scene,
    context: &mut TextContext,
    text: &str,
    font_size: f32,
    origin: Point,
    brush: impl Into<tileink::Brush>,
    attrs: TextAttrs<'_>,
    transform: Affine,
) {
    let layout = context.layout(TextLayoutOptions::new(text, font_size).with_attrs(attrs));
    scene.push_text_layout_as_path(context, &layout, origin, brush, transform, 0.1);
}
