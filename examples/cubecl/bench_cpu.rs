use std::time::{Duration, Instant};

use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect, Shape, Stroke},
};
use tileink::{CubeWgpuRenderer, FillRule, Scene};

fn build_scene(width: u32, height: u32) -> Scene {
    let mut scene = Scene::new(width, height);
    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        tileink::Radius::ZERO,
        Color::from_rgb8(248, 249, 251),
        FillRule::NonZero,
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

fn avg(total: Duration, iterations: u32) -> Duration {
    total / iterations
}

fn checksum(renderer: &CubeWgpuRenderer) -> u64 {
    renderer
        .image()
        .pixels
        .iter()
        .fold(0u64, |sum, &pixel| sum.wrapping_add(pixel as u64))
}

fn main() {
    let width = 1024;
    let height = 1024;
    let warmup = 3;
    let iterations = 20;
    let scene = build_scene(width, height);
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::WHITE);

    for _ in 0..warmup {
        renderer.render(&scene);
    }

    let mut total = Duration::ZERO;

    for _ in 0..iterations {
        let start = Instant::now();
        renderer.render(&scene);
        total += start.elapsed();
    }

    let avg_total = avg(total, iterations);

    println!("cubecl benchmark: {width}x{height}, {iterations} iterations");
    println!("scene: 400 filled circles + 400 stroked circles + background");
    println!("avg total:  {:?}", avg_total);
    println!("checksum:   {}", checksum(&renderer));
}
