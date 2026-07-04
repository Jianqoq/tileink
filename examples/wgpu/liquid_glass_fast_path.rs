use std::time::Duration;

use peniko::Color;
use tileink::{Canvas, WgpuRenderProfileReport, WgpuRenderer};

use crate::common;

#[path = "../common/liquid_glass_fast_path.rs"]
mod fast_path;

const WARMUP_FRAMES: usize = 8;
const PROFILE_FRAMES: usize = 64;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    render_scene("liquid_glass_fast_path_mixed", &fast_path::mixed_scene())?;
    render_scene(
        "liquid_glass_fast_path_default",
        &fast_path::single_mode_scene(fast_path::GlassMode::Default),
    )?;
    render_scene(
        "liquid_glass_fast_path_simple",
        &fast_path::single_mode_scene(fast_path::GlassMode::Simple),
    )?;

    profile_scene(
        "default liquid glass panels=64",
        fast_path::PROFILE_WIDTH,
        fast_path::PROFILE_HEIGHT,
        &fast_path::profile_scene_for_mode(fast_path::GlassMode::Default, 64),
    );
    profile_scene(
        "simple liquid glass panels=64",
        fast_path::PROFILE_WIDTH,
        fast_path::PROFILE_HEIGHT,
        &fast_path::profile_scene_for_mode(fast_path::GlassMode::Simple, 64),
    );
    Ok(())
}

fn render_scene(name: &str, scene: &Canvas) -> Result<(), Box<dyn std::error::Error>> {
    let mut renderer =
        WgpuRenderer::new_default_device(fast_path::WIDTH, fast_path::HEIGHT, Color::WHITE);
    renderer.render(scene);
    let image = renderer.image();
    let out = common::wgpu_example_output(name);
    common::save_example_image(&image, &out)?;
    println!("Wrote {}", out.display());
    Ok(())
}

fn profile_scene(name: &str, width: u32, height: u32, scene: &Canvas) {
    let mut renderer = WgpuRenderer::new_default_device(width, height, Color::WHITE);
    for _ in 0..WARMUP_FRAMES {
        renderer.render(scene);
    }

    let mut report = WgpuRenderProfileReport::new();
    for _ in 0..PROFILE_FRAMES {
        renderer.start_profile();
        renderer.render(scene);
        let profile = renderer.end_profile().clone();
        report.push(&profile);
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
}
