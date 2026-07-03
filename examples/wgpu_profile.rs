use std::env;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect, Shape, Stroke},
};
use tileink::{FillRule, Scene, WgpuRenderProfileReport, WgpuRenderer};

fn main() {
    let width = env_u32("TILEINK_PROFILE_WIDTH", 1024);
    let height = env_u32("TILEINK_PROFILE_HEIGHT", 1024);
    let warmup = env_u32("TILEINK_PROFILE_WARMUP", 3);
    let iterations = env_u32("TILEINK_PROFILE_ITERATIONS", 20);

    let scene = build_scene(width, height);
    let mut renderer = WgpuRenderer::new_default_device(width, height, Color::WHITE);

    for _ in 0..warmup {
        renderer.render(&scene);
    }

    let mut report = WgpuRenderProfileReport::new();
    for _ in 0..iterations {
        renderer.start_profile();
        renderer.render(&scene);
        let profile = renderer.end_profile().clone();
        report.push(&profile);
    }

    println!("wgpu profile: {width}x{height}, {iterations} iterations");
    println!("scene: 400 filled circles + 400 stroked circles + background");
    println!(
        "timestamp query: {}",
        renderer
            .device()
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
    );
    println!("{report}");
}

fn build_scene(width: u32, height: u32) -> Scene {
    let mut scene = Scene::new(width, height);
    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        tileink::Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
    );

    let cols = 20;
    let rows = 20;
    let cell_w = width as f64 / cols as f64;
    let cell_h = height as f64 / rows as f64;

    for row in 0..rows {
        for col in 0..cols {
            let cx = (col as f64 + 0.5) * cell_w;
            let cy = (row as f64 + 0.5) * cell_h;
            let radius = cell_w.min(cell_h) * 0.34;
            let color = Color::from_rgba8(
                (40 + (col * 7) % 180) as u8,
                (80 + (row * 9) % 150) as u8,
                (120 + ((row + col) * 5) % 120) as u8,
                190,
            );

            scene.push_path(
                Circle::new((cx, cy), radius).to_path(0.1),
                color,
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );

            scene.push_stroke(
                Circle::new((cx + cell_w * 0.08, cy - cell_h * 0.06), radius * 0.66),
                Stroke::new(3.0),
                Color::from_rgba8(32, 42, 54, 160),
                Affine::IDENTITY,
                FillRule::NonZero,
                0.1,
            );
        }
    }

    scene
}

fn env_u32(name: &str, fallback: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}
