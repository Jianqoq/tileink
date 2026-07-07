use crate::common;

use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{BlurSampling, Canvas, Filter, Radius, Region};

const WIDTH: u32 = 960;
const HEIGHT: u32 = 540;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut scene = Canvas::new(WIDTH, HEIGHT, 1.0);
    draw_background(&mut scene);
    draw_blur_panel(&mut scene, Rect::new(70.0, 92.0, 430.0, 448.0), 1);
    draw_blur_panel(&mut scene, Rect::new(530.0, 92.0, 890.0, 448.0), 4);
    common::render_to_png_wgpu("downsample_blur", &scene, WIDTH, HEIGHT, Color::WHITE)
}

fn draw_background(scene: &mut Canvas) {
    common::fill_rect(
        scene,
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        Radius::ZERO,
        Color::from_rgb8(245, 247, 250),
    );

    for i in 0..28 {
        let x = 20.0 + f64::from(i) * 34.0;
        let color = match i % 4 {
            0 => Color::from_rgb8(14, 165, 233),
            1 => Color::from_rgb8(244, 63, 94),
            2 => Color::from_rgb8(34, 197, 94),
            _ => Color::from_rgb8(250, 204, 21),
        };
        common::fill_rect(
            scene,
            Rect::new(x, 44.0, x + 16.0, 496.0),
            Radius::all(3.0),
            color,
        );
    }

    draw_reference_shapes(scene, 70.0);
    draw_reference_shapes(scene, 530.0);
}

fn draw_reference_shapes(scene: &mut Canvas, x0: f64) {
    common::fill_circle(
        scene,
        Circle::new((x0 + 158.0, 236.0), 112.0),
        Color::from_rgba8(124, 58, 237, 235),
    );
    common::fill_circle(
        scene,
        Circle::new((x0 + 230.0, 286.0), 96.0),
        Color::from_rgba8(236, 72, 153, 210),
    );
    common::fill_rect(
        scene,
        Rect::new(x0 + 72.0, 310.0, x0 + 308.0, 390.0),
        Radius::all(16.0),
        Color::from_rgba8(34, 197, 94, 220),
    );
}

fn draw_blur_panel(scene: &mut Canvas, panel: Rect, downsample: u32) {
    scene.push_backdrop_layer(
        Filter::Blur {
            std_dev_x: 28.0,
            std_dev_y: 28.0,
            sampling: BlurSampling::downsampled(downsample),
        },
        Region::rect(panel, Radius::all(36.0)),
    );
    common::fill_rect(
        scene,
        panel,
        Radius::all(36.0),
        Color::from_rgba8(255, 255, 255, 96),
    );
    scene.pop_layer();
    common::stroke_rect(
        scene,
        panel,
        Radius::all(36.0),
        peniko::kurbo::Stroke::new(2.0),
        Color::from_rgba8(255, 255, 255, 180),
    );
}
