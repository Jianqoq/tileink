use crate::common;

use peniko::{
    Color,
    kurbo::{Affine, BezPath, Circle, Rect, Shape, Stroke},
};
use tileink::{Canvas, FillRule, Radius};

fn complex_clip_path() -> BezPath {
    let mut path = BezPath::new();
    path.move_to((124.0, 128.0));
    path.curve_to((134.0, 96.0), (166.0, 86.0), (184.0, 102.0));
    path.curve_to((202.0, 74.0), (252.0, 82.0), (270.0, 114.0));
    path.curve_to((304.0, 124.0), (290.0, 164.0), (258.0, 164.0));
    path.curve_to((238.0, 190.0), (194.0, 182.0), (178.0, 158.0));
    path.curve_to((154.0, 178.0), (126.0, 160.0), (124.0, 128.0));
    path.close_path();
    path
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(360, 260, 1.0);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, 360.0, 260.0),
        Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );
    common::stroke_circle(
        &mut scene,
        Circle::new((180.0, 130.0), 74.0),
        Stroke::new(3.0),
        Color::from_rgb8(31, 41, 55),
    );
    scene.push_clip_layer(
        Circle::new((180.0, 130.0), 74.0).to_path(0.1),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    let clip_path = Rect::new(118.0, 82.0, 300.0, 178.0).to_path(0.1);
    let clip_stroke_path = Rect::new(118.5, 82.5, 299.5, 177.5).to_path(0.1);
    let complex = complex_clip_path();
    scene.push_clip_layer(clip_path.clone(), Affine::IDENTITY, FillRule::NonZero, 0.1);
    scene.push_clip_layer(complex.clone(), Affine::IDENTITY, FillRule::NonZero, 0.1);
    common::fill_rect(
        &mut scene,
        Rect::new(64.0, 56.0, 330.0, 204.0),
        Radius::ZERO,
        Color::from_rgba8(14, 165, 233, 235),
    );
    scene.pop_layer();
    scene.pop_layer();
    scene.pop_layer();
    scene.push_stroke(
        clip_stroke_path,
        Stroke::new(3.0),
        Color::from_rgb8(100, 116, 139),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    scene.push_stroke(
        complex,
        Stroke::new(2.0),
        Color::from_rgb8(220, 38, 38),
        Affine::IDENTITY,
        FillRule::NonZero,
        0.1,
    );
    common::render_to_png_wgpu("clip", &scene, 360, 260, Color::WHITE)
}
