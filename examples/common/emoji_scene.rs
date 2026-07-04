use peniko::{
    Color, Gradient,
    color::palette::css,
    kurbo::{Point, Rect},
};
use tileink::{Brush, Canvas, TextAlign, TextContext, TextFontSystem, TextLayoutOptions};

const DESIGN_WIDTH: u32 = 460;
const DESIGN_HEIGHT: u32 = 180;

pub const WIDTH: u32 = crate::common::EXAMPLE_WIDTH;
pub const HEIGHT: u32 = crate::common::EXAMPLE_HEIGHT;
pub const CLEAR: Color = Color::WHITE;

fn offset() -> (f64, f64) {
    (
        (WIDTH as f64 - DESIGN_WIDTH as f64) * 0.5,
        (HEIGHT as f64 - DESIGN_HEIGHT as f64) * 0.5,
    )
}

pub fn scene(font_system: &mut TextFontSystem, context: &mut TextContext) -> Canvas {
    let mut scene = Canvas::new(WIDTH, HEIGHT);
    let (dx, dy) = offset();
    scene.push_rect(
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        tileink::Radius::ZERO,
        CLEAR,
    );

    push_line(
        context,
        font_system,
        &mut scene,
        "Emoji 😀 👍🏽 ✨",
        42.0,
        66.0 + dy,
        Color::BLACK,
    );

    let gradient = Gradient::new_linear((dx as f32 + 48.0, 0.0), (dx as f32 + 412.0, 0.0))
        .with_stops([css::MAGENTA, css::ORANGE, css::DODGER_BLUE]);
    push_line(
        context,
        font_system,
        &mut scene,
        "Family 👨‍👩‍👧‍👦  Flag 🇺🇸",
        30.0,
        116.0 + dy,
        Brush::from_gradient(&gradient),
    );

    push_line(
        context,
        font_system,
        &mut scene,
        "Text fallback stays visible when color emoji is unavailable",
        16.0,
        154.0 + dy,
        Color::from_rgb8(31, 41, 55),
    );

    scene
}

fn push_line(
    context: &mut TextContext,
    font_system: &mut TextFontSystem,
    scene: &mut Canvas,
    text: &str,
    font_size: f32,
    baseline: f64,
    brush: impl Into<Brush>,
) {
    let layout = context.layout(
        font_system,
        TextLayoutOptions::new(text, font_size)
            .with_size(Some(DESIGN_WIDTH as f32 - 32.0), None)
            .with_alignment(Some(TextAlign::Center)),
    );
    if layout.is_empty() {
        return;
    }

    let bounds = layout.bounds();
    let x = (WIDTH as i32 - bounds.width() as i32) / 2 - bounds.x0;
    scene.push_text_layout(&layout, Point::new(x as f64, baseline), brush);
}
