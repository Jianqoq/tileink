#[path = "common/mod.rs"]
mod common;

use std::{sync::mpsc, time::Duration};

use peniko::{
    Color,
    kurbo::{Circle, Rect, Stroke},
};
use tileink::{
    BlurSampling, Canvas, Filter, Radius, RectLiquidGlass, Region, WgpuRenderProfileReport,
};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const WARMUP_FRAMES: usize = 8;
const PROFILE_FRAMES: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    profile_scene("large full-res blur", &large_blur_scene())?;
    profile_scene("full-res liquid glass", &liquid_glass_scene(false))?;
    profile_scene("downsampled liquid glass", &liquid_glass_scene(true))?;
    Ok(())
}

fn large_blur_scene() -> Canvas {
    let mut scene = Canvas::new(WIDTH, HEIGHT, 1.0);
    background(&mut scene);
    scene.push_filter_layer(
        Filter::Blur {
            std_dev_x: 28.0,
            std_dev_y: 28.0,
            sampling: BlurSampling::FULL_RES,
        },
        common::canvas_region(WIDTH, HEIGHT),
    );
    common::fill_circle(
        &mut scene,
        Circle::new((720.0, 430.0), 260.0),
        Color::from_rgba8(37, 99, 235, 230),
    );
    common::fill_rect(
        &mut scene,
        Rect::new(870.0, 360.0, 1310.0, 720.0),
        Radius::ZERO,
        Color::from_rgba8(220, 38, 38, 210),
    );
    scene.pop_layer();
    scene
}

fn liquid_glass_scene(downsampled: bool) -> Canvas {
    let mut scene = Canvas::new(WIDTH, HEIGHT, 1.0);
    background(&mut scene);
    let sampling = if downsampled {
        BlurSampling::downsampled(4)
    } else {
        BlurSampling::FULL_RES
    };
    for row in 0..4 {
        for col in 0..6 {
            let x0 = 68.0 + f64::from(col) * 300.0;
            let y0 = 74.0 + f64::from(row) * 236.0;
            let panel = Rect::new(x0, y0, x0 + 240.0, y0 + 184.0);
            scene.push_backdrop_layer(
                Filter::RectLiquidGlass(RectLiquidGlass {
                    blur_radius: 84,
                    blur_sampling: sampling,
                    tint: Color::from_rgba8(255, 255, 255, 0),
                    ..RectLiquidGlass::default()
                }),
                Region::rect(panel, Radius::all(24.0)),
            );
            scene.pop_layer();
            common::stroke_rect(
                &mut scene,
                panel,
                Radius::all(24.0),
                Stroke::new(2.0),
                Color::from_rgba8(255, 255, 255, 210),
            );
        }
    }
    scene
}

fn background(scene: &mut Canvas) {
    common::fill_rect(
        scene,
        Rect::new(0.0, 0.0, f64::from(WIDTH), f64::from(HEIGHT)),
        Radius::ZERO,
        Color::from_rgb8(245, 247, 250),
    );
    for row in 0..14 {
        for col in 0..22 {
            let x = 24.0 + f64::from(col) * 92.0;
            let y = 22.0 + f64::from(row) * 80.0;
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
            common::fill_circle(scene, Circle::new((x + 66.0, y + 27.0), 11.0), Color::BLACK);
        }
    }
}

fn profile_scene(name: &str, scene: &Canvas) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer = common::new_wgpu_renderer(WIDTH, HEIGHT, Color::WHITE);
    for _ in 0..WARMUP_FRAMES {
        renderer.render(scene);
        wait_for_gpu(renderer.device(), renderer.queue())?;
    }

    let mut report = WgpuRenderProfileReport::new();
    for _ in 0..PROFILE_FRAMES {
        renderer.start_profile();
        renderer.render(scene);
        let _ = renderer.end_profile();
        wait_for_gpu(renderer.device(), renderer.queue())?;
        report.push(&renderer.poll_profile().clone());
    }

    println!("\n{name}");
    for summary in report.profile().summary() {
        if summary.name.starts_with("filter.") {
            let millis = summary
                .gpu_duration
                .map(|duration| duration.as_secs_f64() * 1_000.0 / report.iterations() as f64)
                .unwrap_or_default();
            println!("{:<34} {:>8.3} ms", summary.name, millis);
        }
    }
    let filter_total: Duration = report
        .profile()
        .summary()
        .into_iter()
        .filter(|summary| summary.name.starts_with("filter."))
        .filter_map(|summary| summary.gpu_duration)
        .sum();
    println!(
        "{:<34} {:>8.3} ms",
        "filter.total",
        filter_total.as_secs_f64() * 1_000.0 / report.iterations() as f64
    );
    Ok(())
}

fn wait_for_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel();
    queue.on_submitted_work_done(move || {
        let _ = tx.send(());
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()?;
    Ok(())
}
