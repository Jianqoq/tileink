#![allow(dead_code)]

use peniko::{
    Color,
    kurbo::{Circle, Rect, Stroke},
};
use tileink::{BlurSampling, Filter, Radius, RectLiquidGlass, Region, Scene};

use crate::common;

pub const WIDTH: u32 = 1080;
pub const HEIGHT: u32 = 560;
pub const PROFILE_WIDTH: u32 = 1920;
pub const PROFILE_HEIGHT: u32 = 1080;

#[derive(Clone, Copy)]
pub enum GlassMode {
    Default,
    Simple,
    Blur,
}

pub fn mixed_scene() -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    background(&mut scene, WIDTH, HEIGHT);

    add_panel(
        &mut scene,
        Rect::new(70.0, 112.0, 350.0, 448.0),
        Radius::all(42.0),
        GlassMode::Default,
    );
    add_panel(
        &mut scene,
        Rect::new(400.0, 112.0, 680.0, 448.0),
        Radius::all(42.0),
        GlassMode::Simple,
    );
    add_panel(
        &mut scene,
        Rect::new(730.0, 112.0, 1010.0, 448.0),
        Radius::all(42.0),
        GlassMode::Blur,
    );
    scene
}

pub fn single_mode_scene(mode: GlassMode) -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    background(&mut scene, WIDTH, HEIGHT);
    add_panel(
        &mut scene,
        Rect::new(200.0, 96.0, 860.0, 420.0),
        Radius::all(48.0),
        mode,
    );
    scene
}

pub fn profile_scene_for_mode(mode: GlassMode, panels: u32) -> Scene {
    let mut scene = Scene::new(PROFILE_WIDTH, PROFILE_HEIGHT);
    background(&mut scene, PROFILE_WIDTH, PROFILE_HEIGHT);
    let columns = if panels <= 16 { 4 } else { 8 };
    let rows = panels.div_ceil(columns);
    let gap = 20.0;
    let margin = 44.0;
    let panel_width =
        (PROFILE_WIDTH as f64 - margin * 2.0 - gap * f64::from(columns - 1)) / f64::from(columns);
    let panel_height =
        (PROFILE_HEIGHT as f64 - margin * 2.0 - gap * f64::from(rows - 1)) / f64::from(rows);

    for i in 0..panels {
        let col = i % columns;
        let row = i / columns;
        let x0 = margin + f64::from(col) * (panel_width + gap);
        let y0 = margin + f64::from(row) * (panel_height + gap);
        add_panel(
            &mut scene,
            Rect::new(x0, y0, x0 + panel_width, y0 + panel_height),
            Radius::all(18.0),
            mode,
        );
    }
    scene
}

fn background(scene: &mut Scene, width: u32, height: u32) {
    common::fill_rect(
        scene,
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        Radius::ZERO,
        Color::from_rgb8(245, 247, 250),
    );

    let cols = (width / 94).max(1);
    let rows = (height / 82).max(1);
    for row in 0..=rows {
        for col in 0..=cols {
            let x = 26.0 + f64::from(col) * 94.0;
            let y = 24.0 + f64::from(row) * 82.0;
            let color = match (row + col) % 5 {
                0 => Color::from_rgb8(37, 99, 235),
                1 => Color::from_rgb8(244, 63, 94),
                2 => Color::from_rgb8(34, 197, 94),
                3 => Color::from_rgb8(250, 204, 21),
                _ => Color::from_rgb8(124, 58, 237),
            };
            common::fill_rect(
                scene,
                Rect::new(x, y, x + 54.0, y + 54.0),
                Radius::all(8.0),
                color,
            );
            common::fill_circle(
                scene,
                Circle::new((x + 66.0, y + 27.0), 11.0),
                Color::from_rgb8(15, 23, 42),
            );
        }
    }
}

fn add_panel(scene: &mut Scene, panel: Rect, radius: Radius, mode: GlassMode) {
    match mode {
        GlassMode::Default => scene.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 16,
                blur_sampling: BlurSampling::downsampled(4),
                tint: Color::from_rgba8(255, 255, 255, 0),
                ..RectLiquidGlass::default()
            }),
            Region::rect(panel, radius),
        ),
        GlassMode::Simple => scene.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 16,
                blur_sampling: BlurSampling::downsampled(4),
                tint: Color::from_rgba8(255, 255, 255, 0),
                refraction_dispersion: 0.0,
                fresnel_factor: 0.0,
                glare_factor: 0.0,
                ..RectLiquidGlass::default()
            }),
            Region::rect(panel, radius),
        ),
        GlassMode::Blur => scene.push_backdrop_layer(
            Filter::Blur {
                std_dev_x: 18.0,
                std_dev_y: 18.0,
                sampling: BlurSampling::downsampled(4),
            },
            Region::rect(panel, radius),
        ),
    }
    scene.pop_layer();
    common::stroke_rect(
        scene,
        panel,
        radius,
        Stroke::new(2.0),
        Color::from_rgba8(255, 255, 255, 210),
    );
}
