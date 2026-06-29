use peniko::{Color, kurbo::Point};
use tileink::{Scene, TextAlign, TextContext, TextLayoutOptions};

pub const WIDTH: u32 = 280;
pub const HEIGHT: u32 = 88;

pub struct TextCase {
    pub name: &'static str,
    pub font_size: f32,
    pub foreground: Color,
    pub background: Color,
}

pub const CASES: [TextCase; 6] = [
    TextCase {
        name: "text_black_on_white_12px",
        font_size: 12.0,
        foreground: Color::BLACK,
        background: Color::WHITE,
    },
    TextCase {
        name: "text_black_on_white_16px",
        font_size: 16.0,
        foreground: Color::BLACK,
        background: Color::WHITE,
    },
    TextCase {
        name: "text_black_on_white_24px",
        font_size: 24.0,
        foreground: Color::BLACK,
        background: Color::WHITE,
    },
    TextCase {
        name: "text_white_on_black_12px",
        font_size: 12.0,
        foreground: Color::WHITE,
        background: Color::BLACK,
    },
    TextCase {
        name: "text_white_on_black_16px",
        font_size: 16.0,
        foreground: Color::WHITE,
        background: Color::BLACK,
    },
    TextCase {
        name: "text_white_on_black_24px",
        font_size: 24.0,
        foreground: Color::WHITE,
        background: Color::BLACK,
    },
];

pub fn scene(context: &mut TextContext, case: &TextCase) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    let text = format!("Tileink text {}px", case.font_size as u32);
    let layout = context.layout(
        TextLayoutOptions::new(&text, case.font_size)
            .with_size(Some(240.0), None)
            .with_alignment(Some(TextAlign::Center)),
    );
    let bounds = layout.bounds();
    let x = (WIDTH as i32 - bounds.width() as i32) / 2 - bounds.x0;
    let y = (HEIGHT as i32 + bounds.height() as i32) / 2 - bounds.y1;
    scene.push_text_layout(&layout, Point::new(x as f64, y as f64), case.foreground);
    scene
}
