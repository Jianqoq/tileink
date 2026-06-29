use peniko::{
    Color, Extend, Gradient,
    color::palette::css,
    kurbo::{Affine, Point, Rect},
};
use tileink::{
    FillRule, Scene, TextAttrs, TextCacheKeyFlags, TextContext, TextLayoutOptions, TextWeight,
};

pub const WIDTH: u32 = 780;
pub const HEIGHT: u32 = 400;
pub const CLEAR: Color = Color::from_rgb8(248, 250, 252);

pub fn scene(context: &mut TextContext) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.push_rect(
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        CLEAR,
        FillRule::NonZero,
    );

    push_bitmap(
        &mut scene,
        context,
        "Bitmap text: swash raster, hinted, subpixel",
        16.0,
        Point::new(28.0, 36.0),
        Color::from_rgb8(15, 23, 42),
        TextAttrs::new().weight(TextWeight::SEMIBOLD),
    );

    push_bitmap(
        &mut scene,
        context,
        "Hamburgefonts 12px",
        12.0,
        Point::new(28.0, 82.0),
        Color::BLACK,
        TextAttrs::new(),
    );
    push_label(
        &mut scene,
        context,
        "bitmap raster",
        Point::new(220.0, 82.0),
    );

    push_path_text(
        &mut scene,
        context,
        "Hamburgefonts 12px",
        12.0,
        Point::new(28.0, 124.0),
        Color::BLACK,
        TextAttrs::new(),
        Affine::IDENTITY,
    );
    push_label(
        &mut scene,
        context,
        "path outline, hinting on",
        Point::new(220.0, 124.0),
    );

    push_path_text(
        &mut scene,
        context,
        "Hamburgefonts 12px",
        12.0,
        Point::new(28.0, 166.0),
        Color::BLACK,
        TextAttrs::new().cache_key_flags(TextCacheKeyFlags::DISABLE_HINTING),
        Affine::IDENTITY,
    );
    push_label(
        &mut scene,
        context,
        "path outline, hinting off",
        Point::new(220.0, 166.0),
    );

    let gradient = Gradient::new_linear((24.0, 205.0), (610.0, 205.0))
        .with_extend(Extend::Pad)
        .with_stops([css::CRIMSON, css::ORANGE, css::DODGER_BLUE]);
    push_path_text(
        &mut scene,
        context,
        "VECTOR PATH",
        62.0,
        Point::new(28.0, 252.0),
        &gradient,
        TextAttrs::new().weight(TextWeight::BOLD),
        Affine::rotate_about(-5.0_f64.to_radians(), (330.0, 226.0)),
    );

    push_path_text(
        &mut scene,
        context,
        "Affine transform + gradient brush",
        25.0,
        Point::new(30.0, 362.0),
        Color::from_rgb8(15, 118, 110),
        TextAttrs::new().weight(TextWeight::SEMIBOLD),
        Affine::rotate_about(4.0_f64.to_radians(), (240.0, 352.0)),
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
