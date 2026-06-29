use peniko::{
    Color, Gradient,
    color::palette::css,
    kurbo::{Point, Rect},
};
use tileink::{Brush, FillRule, Scene, TextAlign, TextContext, TextLayoutOptions};

pub const WIDTH: u32 = 460;
pub const HEIGHT: u32 = 180;
pub const CLEAR: Color = Color::WHITE;

pub fn scene(context: &mut TextContext) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.push_rect(
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        CLEAR,
        FillRule::NonZero,
    );

    push_line(
        context,
        &mut scene,
        "Emoji 😀 👍🏽 ✨",
        42.0,
        66.0,
        Color::BLACK,
    );

    let gradient = Gradient::new_linear((48.0, 0.0), (WIDTH as f32 - 48.0, 0.0)).with_stops([
        css::MAGENTA,
        css::ORANGE,
        css::DODGER_BLUE,
    ]);
    push_line(
        context,
        &mut scene,
        "Family 👨‍👩‍👧‍👦  Flag 🇺🇸",
        30.0,
        116.0,
        Brush::from_gradient(&gradient),
    );

    push_line(
        context,
        &mut scene,
        "Text fallback stays visible when color emoji is unavailable",
        16.0,
        154.0,
        Color::from_rgb8(31, 41, 55),
    );

    scene
}

fn push_line(
    context: &mut TextContext,
    scene: &mut Scene,
    text: &str,
    font_size: f32,
    baseline: f64,
    brush: impl Into<Brush>,
) {
    let layout = context.layout(
        TextLayoutOptions::new(text, font_size)
            .with_size(Some(WIDTH as f32 - 32.0), None)
            .with_alignment(Some(TextAlign::Center)),
    );
    if layout.is_empty() {
        return;
    }

    let bounds = layout.bounds();
    let x = (WIDTH as i32 - bounds.width() as i32) / 2 - bounds.x0;
    scene.push_text_layout(&layout, Point::new(x as f64, baseline), brush);
}
