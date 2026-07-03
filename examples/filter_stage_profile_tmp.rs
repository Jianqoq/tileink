use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use peniko::{
    Color,
    kurbo::{Circle, Rect},
};
use tileink::{
    BlurSampling, Filter, Radius, RectLiquidGlass, Region, Scene, WgpuRenderProfileReport,
    WgpuRenderer,
};

#[path = "common/mod.rs"]
mod common;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const WARMUP_FRAMES: usize = 8;
const PROFILE_FRAMES: usize = 64;

fn main() {
    run_case("blur panels=16", blur_scene(16));
    run_case("blur panels=64", blur_scene(64));
    run_case("glass panels=16", glass_scene(16));
    run_case("glass panels=64", glass_scene(64));
    run_case("simple glass panels=16", simple_glass_scene(16));
    run_case("simple glass panels=64", simple_glass_scene(64));
}

fn run_case(name: &str, scene: Scene) {
    let mut renderer = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::WHITE);
    for _ in 0..WARMUP_FRAMES {
        renderer.render(&scene);
        wait_for_gpu(&renderer);
    }

    let render_wait_avg = bench_render_wait(&mut renderer, &scene);
    let mut report = WgpuRenderProfileReport::new();
    for _ in 0..PROFILE_FRAMES {
        renderer.start_profile();
        renderer.render(&scene);
        let profile = renderer.end_profile().clone();
        report.push(&profile);
    }

    println!("\n{name}");
    println!("{:<34} {:>8.3} ms", "render.wait.avg", render_wait_avg);
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
}

fn bench_render_wait(renderer: &mut WgpuRenderer, scene: &Scene) -> f64 {
    let mut total = Duration::ZERO;
    for _ in 0..PROFILE_FRAMES {
        let start = Instant::now();
        renderer.render(scene);
        wait_for_gpu(renderer);
        total += start.elapsed();
    }
    total.as_secs_f64() * 1_000.0 / PROFILE_FRAMES as f64
}

fn wait_for_gpu(renderer: &WgpuRenderer) {
    let (tx, rx) = mpsc::channel();
    renderer.queue().on_submitted_work_done(move || {
        let _ = tx.send(());
    });
    renderer
        .device()
        .poll(::wgpu::PollType::wait_indefinitely())
        .expect("poll wgpu device");
    rx.recv().expect("receive wgpu completion");
}

fn blur_scene(panels: u32) -> Scene {
    let mut scene = background_scene();
    for panel in panel_grid(panels) {
        scene.push_backdrop_layer(
            Filter::Blur {
                std_dev_x: 18.0,
                std_dev_y: 18.0,
                sampling: BlurSampling::downsampled(4),
            },
            Region::rect(panel, Radius::all(18.0)),
        );
        common::fill_rect(
            &mut scene,
            panel,
            Radius::all(18.0),
            Color::from_rgba8(255, 255, 255, 88),
        );
        scene.pop_layer();
    }
    scene
}

fn glass_scene(panels: u32) -> Scene {
    let mut scene = background_scene();
    for panel in panel_grid(panels) {
        scene.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 16,
                blur_sampling: BlurSampling::downsampled(4),
                tint: Color::from_rgba8(255, 255, 255, 0),
                ..RectLiquidGlass::default()
            }),
            Region::rect(panel, Radius::all(18.0)),
        );
        scene.pop_layer();
    }
    scene
}

fn simple_glass_scene(panels: u32) -> Scene {
    let mut scene = background_scene();
    for panel in panel_grid(panels) {
        scene.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 16,
                blur_sampling: BlurSampling::downsampled(4),
                tint: Color::from_rgba8(255, 255, 255, 0),
                refraction_dispersion: 0.0,
                fresnel_factor: 0.0,
                glare_factor: 0.0,
                ..RectLiquidGlass::default()
            }),
            Region::rect(panel, Radius::all(18.0)),
        );
        scene.pop_layer();
    }
    scene
}

fn background_scene() -> Scene {
    let mut scene = Scene::new(WIDTH, HEIGHT);
    common::fill_rect(
        &mut scene,
        Rect::new(0.0, 0.0, WIDTH as f64, HEIGHT as f64),
        Radius::ZERO,
        Color::from_rgb8(245, 247, 250),
    );
    for row in 0..9 {
        for col in 0..16 {
            let x = 32.0 + col as f64 * 116.0;
            let y = 28.0 + row as f64 * 112.0;
            let color = match (row + col) % 4 {
                0 => Color::from_rgb8(37, 99, 235),
                1 => Color::from_rgb8(244, 63, 94),
                2 => Color::from_rgb8(34, 197, 94),
                _ => Color::from_rgb8(250, 204, 21),
            };
            common::fill_rect(
                &mut scene,
                Rect::new(x, y, x + 72.0, y + 72.0),
                Radius::all(8.0),
                color,
            );
            common::fill_circle(
                &mut scene,
                Circle::new((x + 86.0, y + 36.0), 14.0),
                Color::from_rgb8(15, 23, 42),
            );
        }
    }
    scene
}

fn panel_grid(count: u32) -> impl Iterator<Item = Rect> {
    let columns = if count <= 16 { 4 } else { 8 };
    let rows = count.div_ceil(columns);
    let gap = 20.0;
    let margin = 44.0;
    let panel_width = (WIDTH as f64 - margin * 2.0 - gap * (columns - 1) as f64) / columns as f64;
    let panel_height = (HEIGHT as f64 - margin * 2.0 - gap * (rows - 1) as f64) / rows as f64;
    (0..count).map(move |i| {
        let col = i % columns;
        let row = i / columns;
        let x0 = margin + col as f64 * (panel_width + gap);
        let y0 = margin + row as f64 * (panel_height + gap);
        Rect::new(x0, y0, x0 + panel_width, y0 + panel_height)
    })
}
