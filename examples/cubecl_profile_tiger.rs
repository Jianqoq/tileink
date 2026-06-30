#[path = "common/mod.rs"]
mod common;

use peniko::Color;
use tileink::CubeWgpuRenderer;

const TARGET_WIDTH: u32 = 900;
const WARMUP: usize = 3;
const ITERATIONS: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = common::example_asset("tiger.svg");
    let (scene, width, height) = common::load_svg_scene(input, TARGET_WIDTH)?;
    let mut renderer = CubeWgpuRenderer::new_default_device(width, height, Color::TRANSPARENT);

    for _ in 0..WARMUP {
        renderer.render(&scene);
    }

    println!("tiger profile: {width}x{height}, warmup={WARMUP}, iterations={ITERATIONS}");
    let mut wall_total = std::time::Duration::ZERO;
    let mut event_totals = Vec::<(
        &'static str,
        std::time::Duration,
        Option<std::time::Duration>,
    )>::new();
    for _ in 0..ITERATIONS {
        renderer.start_profile();
        renderer.render(&scene);
        let profile = renderer.end_profile();
        wall_total += profile.wall_time();
        for entry in profile.entries() {
            if let Some((_, wall, kernel)) = event_totals
                .iter_mut()
                .find(|(name, _, _)| *name == entry.name)
            {
                *wall += entry.duration;
                *kernel = merge_optional_duration(*kernel, entry.kernel_duration);
            } else {
                event_totals.push((entry.name, entry.duration, entry.kernel_duration));
            }
        }
    }

    let attributed_total = event_totals
        .iter()
        .map(|(_, duration, _)| *duration)
        .sum::<std::time::Duration>();
    let kernel_total = event_totals
        .iter()
        .filter_map(|(_, _, duration)| *duration)
        .sum::<std::time::Duration>();
    println!(
        "{:<36} {:>12} {:>12} {:>10} {:>10} {:>9}",
        "event", "kernel us", "wall us", "kernel %", "event %", "wall %"
    );
    for (name, duration, kernel_duration) in &event_totals {
        let kernel_micros = kernel_duration
            .map(|duration| format!("{:.3}", micros(duration) / ITERATIONS as f64))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{name:<36} {:>12} {:>12.3} {:>9.2}% {:>9.2}% {:>8.2}%",
            kernel_micros,
            micros(*duration) / ITERATIONS as f64,
            kernel_duration
                .map(|duration| percent(duration, kernel_total))
                .unwrap_or(0.0),
            percent(*duration, attributed_total),
            percent(*duration, wall_total)
        );
    }
    let unattributed = wall_total
        .checked_sub(attributed_total)
        .unwrap_or(std::time::Duration::ZERO);
    if unattributed > std::time::Duration::ZERO {
        println!(
            "{:<36} {:>12} {:>12.3} {:>10} {:>9} {:>8.2}%",
            "unattributed",
            "",
            micros(unattributed) / ITERATIONS as f64,
            "",
            "",
            percent(unattributed, wall_total)
        );
    }
    println!(
        "{:<36} {:>12.3} {:>12.3} {:>10} {:>9} {:>8.2}%",
        "total",
        micros(kernel_total) / ITERATIONS as f64,
        micros(wall_total) / ITERATIONS as f64,
        "",
        "",
        100.0
    );

    Ok(())
}

fn micros(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

fn merge_optional_duration(
    left: Option<std::time::Duration>,
    right: Option<std::time::Duration>,
) -> Option<std::time::Duration> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left + right),
        (Some(duration), None) | (None, Some(duration)) => Some(duration),
        (None, None) => None,
    }
}

fn percent(duration: std::time::Duration, total: std::time::Duration) -> f64 {
    if total.is_zero() {
        0.0
    } else {
        duration.as_secs_f64() * 100.0 / total.as_secs_f64()
    }
}
