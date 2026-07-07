use peniko::{
    Color, Extend, Gradient,
    color::{AlphaColor, palette::css},
    kurbo::{Affine, BezPath, Circle, Rect},
};
use tileink::{Brush, Canvas, FillRule, Radius};

use crate::common::{fill_circle, fill_rect};

pub const WIDTH: u32 = 720;
pub const HEIGHT: u32 = 520;
pub const CLEAR: Color = Color::from_rgb8(245, 247, 250);

pub fn scene() -> Canvas {
    let mut scene = Canvas::new(WIDTH, HEIGHT, 1.0);

    let linear = Gradient::new_linear((36.0, 0.0), (324.0, 0.0))
        .with_extend(Extend::Pad)
        .with_stops([css::RED, css::ORANGE, css::BLUE]);
    fill_rect(
        &mut scene,
        Rect::new(36.0, 36.0, 324.0, 136.0),
        Radius::all(18.0),
        &linear,
    );

    let radial =
        Gradient::new_radial((520.0, 86.0), 94.0).with_stops([css::WHITE, css::DODGER_BLUE]);
    fill_circle(&mut scene, Circle::new((520.0, 86.0), 72.0), &radial);

    let two_point = Gradient::new_two_point_radial((104.0, 260.0), 8.0, (242.0, 240.0), 128.0)
        .with_stops([css::YELLOW, css::MAGENTA, css::DARK_BLUE]);
    fill_rect(
        &mut scene,
        Rect::new(36.0, 174.0, 324.0, 330.0),
        Radius::all(24.0),
        &two_point,
    );

    let sweep = Gradient::new_sweep((520.0, 252.0), 0.0, std::f32::consts::TAU).with_stops([
        css::RED,
        css::LIME,
        css::BLUE,
        css::RED,
    ]);
    fill_circle(&mut scene, Circle::new((520.0, 252.0), 78.0), &sweep);

    scene.push_path(
        star_path(180.0, 424.0, 94.0, 43.0, 5),
        Brush::four_corner(
            Rect::new(86.0, 330.0, 274.0, 518.0),
            [
                AlphaColor::from_rgb8(255, 0, 0),
                AlphaColor::from_rgb8(0, 255, 0),
                AlphaColor::from_rgb8(0, 0, 255),
                AlphaColor::from_rgb8(255, 255, 0),
            ],
        ),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );

    fill_rect(
        &mut scene,
        Rect::new(392.0, 364.0, 648.0, 484.0),
        Radius::all(24.0),
        Brush::four_corner(
            Rect::new(392.0, 364.0, 648.0, 484.0),
            [
                AlphaColor::from_rgb8(255, 255, 255),
                AlphaColor::from_rgb8(0, 255, 255),
                AlphaColor::from_rgb8(0, 0, 255),
                AlphaColor::from_rgb8(255, 0, 255),
            ],
        ),
    );

    scene
}

fn star_path(cx: f64, cy: f64, outer: f64, inner: f64, points: usize) -> BezPath {
    let mut path = BezPath::new();
    for ix in 0..points * 2 {
        let angle = -std::f64::consts::FRAC_PI_2 + ix as f64 * std::f64::consts::PI / points as f64;
        let radius = if ix % 2 == 0 { outer } else { inner };
        let point = (cx + angle.cos() * radius, cy + angle.sin() * radius);
        if ix == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close_path();
    path
}
