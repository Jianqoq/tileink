use peniko::{
    Color,
    kurbo::{Circle, Point, Rect, Stroke},
};
use tileink::{FillRule, Filter, Radius, Region, Scene, SdfLine, SdfLineCap};

use crate::common::{fill_circle, fill_rect, stroke_circle, stroke_rect};

pub fn sdf_nested_clip_blur_scene() -> (Scene, u32, u32) {
    let width = 760;
    let height = 460;
    let mut scene = Scene::new(width, height);

    fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        Radius::ZERO,
        Color::from_rgb8(245, 247, 250),
    );

    for x in (48..=712).step_by(48) {
        scene.push_line(
            SdfLine::new(
                Point::new(x as f64 + 0.5, 42.0),
                Point::new(x as f64 + 0.5, 418.0),
                1.0,
                SdfLineCap::Butt,
            ),
            Color::from_rgba8(148, 163, 184, 80),
            FillRule::NonZero,
        );
    }
    for y in (48..=408).step_by(48) {
        scene.push_line(
            SdfLine::new(
                Point::new(48.0, y as f64 + 0.5),
                Point::new(712.0, y as f64 + 0.5),
                1.0,
                SdfLineCap::Butt,
            ),
            Color::from_rgba8(148, 163, 184, 80),
            FillRule::NonZero,
        );
    }

    let rect_clip = Rect::new(96.0, 68.0, 664.0, 392.0);
    let rect_radius = Radius {
        top_left: 56.0,
        top_right: 18.0,
        bottom_left: 28.0,
        bottom_right: 74.0,
    };
    let circle_clip = Circle::new((388.0, 230.0), 156.0);

    scene.push_clip_sdf_rect_layer(rect_clip, rect_radius);
    scene.push_clip_sdf_circle_layer(circle_clip);
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 12.0,
            std_dev_y: 12.0,
        },
        Region::rect(rect_clip, rect_radius),
    );

    fill_rect(
        &mut scene,
        Rect::new(92.0, 58.0, 668.0, 172.0),
        Radius::ZERO,
        Color::from_rgba8(14, 165, 233, 245),
    );
    fill_rect(
        &mut scene,
        Rect::new(92.0, 172.0, 668.0, 282.0),
        Radius::ZERO,
        Color::from_rgba8(34, 197, 94, 245),
    );
    fill_rect(
        &mut scene,
        Rect::new(92.0, 282.0, 668.0, 402.0),
        Radius::ZERO,
        Color::from_rgba8(249, 115, 22, 245),
    );
    fill_circle(
        &mut scene,
        Circle::new((270.0, 156.0), 94.0),
        Color::from_rgba8(255, 255, 255, 180),
    );
    fill_circle(
        &mut scene,
        Circle::new((520.0, 310.0), 118.0),
        Color::from_rgba8(15, 23, 42, 130),
    );
    scene.push_line(
        SdfLine::new(
            Point::new(138.0, 356.0),
            Point::new(636.0, 112.0),
            24.0,
            SdfLineCap::Round,
        ),
        Color::from_rgba8(255, 255, 255, 200),
        FillRule::NonZero,
    );

    scene.pop_layer();
    scene.pop_layer();
    scene.pop_layer();

    stroke_rect(
        &mut scene,
        rect_clip,
        rect_radius,
        Stroke::new(4.0),
        Color::from_rgb8(15, 23, 42),
    );
    stroke_circle(
        &mut scene,
        circle_clip,
        Stroke::new(3.0),
        Color::from_rgba8(255, 255, 255, 235),
    );

    (scene, width, height)
}
