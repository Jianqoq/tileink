use std::time::Duration;

use peniko::{
    Color,
    kurbo::{Affine, Circle, Rect, Shape, Stroke},
};
use tileink::{CpuRenderer, FillRule, Canvas};

fn build_scene(width: u32, height: u32) -> Canvas {
    let mut scene = Canvas::new(width, height);
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

fn avg(total: Duration, iterations: u32) -> Duration {
    total / iterations
}

fn checksum(renderer: &CpuRenderer) -> u64 {
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
    let mut renderer = CpuRenderer::new(width, height, Color::WHITE);

    for _ in 0..warmup {
        renderer.render_profiled_flat(&scene);
    }

    let mut scan = Duration::ZERO;
    let mut cumsum = Duration::ZERO;
    let mut coarse = Duration::ZERO;
    let mut fine = Duration::ZERO;
    let mut total = Duration::ZERO;

    for _ in 0..iterations {
        let profile = renderer.render_profiled_flat(&scene);
        scan += profile.scan;
        cumsum += profile.cumsum;
        coarse += profile.coarse;
        fine += profile.fine;
        total += profile.total;
    }

    let avg_total = avg(total, iterations);
    let avg_fine = avg(fine, iterations);
    let fine_pct = avg_fine.as_secs_f64() / avg_total.as_secs_f64() * 100.0;

    println!("cpu benchmark: {width}x{height}, {iterations} iterations");
    println!("scene: 400 filled circles + 400 stroked circles + background");
    println!("avg total:  {:?}", avg_total);
    println!("avg scan:   {:?}", avg(scan, iterations));
    println!("avg cumsum: {:?}", avg(cumsum, iterations));
    println!("avg coarse: {:?}", avg(coarse, iterations));
    println!("avg fine:   {:?} ({fine_pct:.1}%)", avg_fine);
    println!("checksum:   {}", checksum(&renderer));
}
