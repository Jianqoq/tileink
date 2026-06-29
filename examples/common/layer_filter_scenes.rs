// Shared scenes are compiled into multiple examples; each example selects one
// scene and leaves the others unused in that binary.
#![allow(dead_code)]

use peniko::{
    Color,
    kurbo::{Circle, Rect, Shape, Stroke},
};
use tileink::{FillRule, Filter, LiquidGlass, Radius, Region, Scene};

use crate::common::{fill_circle, fill_rect, stroke_circle, stroke_rect};

pub const WIDTH: u32 = 720;
pub const HEIGHT: u32 = 420;
pub const CLEAR: Color = Color::WHITE;

fn background(scene: &mut Scene) {
    fill_rect(
        scene,
        Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)),
        Radius::all(0.0),
        Color::from_rgb8(245, 247, 250),
    );

    for row in 0..5 {
        for col in 0..9 {
            let x = 34.0 + f64::from(col) * 82.0;
            let y = 34.0 + f64::from(row) * 82.0;
            let color = if (row + col) % 2 == 0 {
                Color::from_rgb8(37, 99, 235)
            } else {
                Color::from_rgb8(249, 115, 22)
            };
            fill_rect(
                scene,
                Rect::new(x, y, x + 48.0, y + 48.0),
                Radius::all(5.0),
                color,
            );
            fill_circle(
                scene,
                Circle::new((x + 58.0, y + 24.0), 10.0),
                Color::from_rgb8(15, 23, 42),
            );
        }
    }
}

pub fn filter_clip_opacity_scene() -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    background(&mut scene);

    let clip = Rect::new(150.0, 82.0, 570.0, 338.0);
    scene.push_clip_layer(
        crate::common::rect_path(clip, Radius::all(54.0)),
        Default::default(),
        FillRule::NonZero,
        0.1,
    );
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 10.0,
            std_dev_y: 10.0,
        },
        Region::rect(clip, Radius::all(54.0)),
    );
    scene.push_opacity_layer(
        crate::common::rect_path(clip, Radius::all(54.0)),
        Default::default(),
        0.1,
        0.62,
    );
    fill_circle(
        &mut scene,
        Circle::new((292.0, 210.0), 128.0),
        Color::from_rgb8(34, 197, 94),
    );
    fill_rect(
        &mut scene,
        Rect::new(328.0, 104.0, 612.0, 318.0),
        Radius::all(28.0),
        Color::from_rgb8(220, 38, 38),
    );
    scene.pop_layer();
    scene.pop_layer();
    scene.pop_layer();

    stroke_rect(
        &mut scene,
        clip,
        Radius::all(54.0),
        Stroke::new(3.0),
        Color::from_rgb8(15, 23, 42),
    );
    scene
}

pub fn clip_filter_scene() -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    background(&mut scene);

    let clip = Circle::new((360.0, 210.0), 142.0);
    scene.push_clip_layer(
        clip.to_path(0.1),
        Default::default(),
        FillRule::NonZero,
        0.1,
    );
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 14.0,
            std_dev_y: 14.0,
        },
        Region::path(clip.to_path(0.1), Default::default(), 0.1),
    );
    fill_rect(
        &mut scene,
        Rect::new(116.0, 92.0, 604.0, 328.0),
        Radius::all(18.0),
        Color::from_rgba8(124, 58, 237, 235),
    );
    fill_circle(
        &mut scene,
        Circle::new((250.0, 188.0), 92.0),
        Color::from_rgba8(250, 204, 21, 240),
    );
    fill_circle(
        &mut scene,
        Circle::new((470.0, 232.0), 108.0),
        Color::from_rgba8(6, 182, 212, 230),
    );
    scene.pop_layer();
    scene.pop_layer();

    stroke_circle(
        &mut scene,
        clip,
        Stroke::new(3.0),
        Color::from_rgb8(15, 23, 42),
    );
    scene
}

pub fn backdrop_blur_scene() -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    background(&mut scene);

    for i in 0..18 {
        let x = 74.0 + f64::from(i) * 34.0;
        let color = if i % 2 == 0 {
            Color::from_rgb8(14, 165, 233)
        } else {
            Color::from_rgb8(244, 63, 94)
        };
        fill_rect(
            &mut scene,
            Rect::new(x, 70.0, x + 10.0, 350.0),
            Radius::all(2.0),
            color,
        );
    }
    fill_circle(
        &mut scene,
        Circle::new((250.0, 210.0), 86.0),
        Color::from_rgb8(250, 204, 21),
    );
    fill_rect(
        &mut scene,
        Rect::new(388.0, 106.0, 570.0, 314.0),
        Radius::all(18.0),
        Color::from_rgb8(34, 197, 94),
    );

    let panel = Rect::new(142.0, 112.0, 578.0, 308.0);
    scene.push_backdrop_layer(
        Filter::Blur {
            std_dev_x: 18.0,
            std_dev_y: 18.0,
        },
        Region::rect(panel, Radius::all(34.0)),
    );
    fill_rect(
        &mut scene,
        panel,
        Radius::all(34.0),
        Color::from_rgba8(255, 255, 255, 150),
    );
    scene.pop_layer();

    stroke_rect(
        &mut scene,
        panel,
        Radius::all(34.0),
        Stroke::new(2.0),
        Color::from_rgba8(255, 255, 255, 220),
    );
    scene
}

pub fn liquid_glass_scene() -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    background(&mut scene);

    for i in 0..20 {
        let x = 48.0 + f64::from(i) * 32.0;
        let color = match i % 3 {
            0 => Color::from_rgb8(236, 72, 153),
            1 => Color::from_rgb8(14, 165, 233),
            _ => Color::from_rgb8(250, 204, 21),
        };
        fill_rect(
            &mut scene,
            Rect::new(x, 52.0, x + 12.0, 368.0),
            Radius::all(6.0),
            color,
        );
    }
    fill_circle(
        &mut scene,
        Circle::new((244.0, 210.0), 92.0),
        Color::from_rgb8(124, 58, 237),
    );
    fill_circle(
        &mut scene,
        Circle::new((500.0, 198.0), 74.0),
        Color::from_rgb8(34, 197, 94),
    );

    let panel = Rect::new(126.0, 104.0, 594.0, 316.0);
    scene.push_backdrop_layer(
        Filter::LiquidGlass(LiquidGlass {
            blur_std_dev: 1.0,
            tint: Color::from_rgba8(255, 255, 255, 0),
            ..LiquidGlass::default()
        }),
        Region::rect(panel, Radius::all(42.0)),
    );
    scene.pop_layer();
    stroke_rect(
        &mut scene,
        panel,
        Radius::all(42.0),
        Stroke::new(2.0),
        Color::from_rgba8(255, 255, 255, 180),
    );
    scene
}
