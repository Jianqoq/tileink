#[path = "support/retained_bench.rs"]
mod retained_bench;
#[path = "support/retained_dirty_ratio.rs"]
mod retained_dirty_ratio;

use std::error::Error;

use peniko::Color;
use retained_bench::{BenchConfig, HEIGHT, WIDTH, bench, bench_persistent, median_ms, ms};
use retained_dirty_ratio::{BACKGROUND_NODES, RATIOS, Workload};
use tileink::{IncrementalRenderMode, WgpuRenderer};

fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    let workload = Workload::new();
    println!(
        "retained dirty-ratio bench: {WIDTH}x{HEIGHT}, {BACKGROUND_NODES} unchanged background nodes, warmup {}, measured frames {}",
        config.warmup, config.frames
    );
    println!(
        "{:<10} {:>12} {:>13} {:>18} {:>12} {:>10} {:>10}",
        "changed",
        "persistent",
        "force-full",
        "preflat immediate",
        "materialize",
        "prepare",
        "GPU KiB"
    );

    for requested_ratio in RATIOS {
        let persistent = bench_persistent(
            &seed,
            config,
            workload.persistent_scene(requested_ratio),
            IncrementalRenderMode::Auto,
            |scene, frame| workload.mutate_persistent(scene, requested_ratio, frame),
        )?;
        let persistent_full = bench_persistent(
            &seed,
            config,
            workload.persistent_scene(requested_ratio),
            IncrementalRenderMode::ForceFull,
            |scene, frame| workload.mutate_persistent(scene, requested_ratio, frame),
        )?;
        let immediate = workload.immediate_frames(requested_ratio);
        let non_retained = bench(&seed, config, &immediate, IncrementalRenderMode::Auto)?;
        let changed_ratio =
            persistent.changed_tiles as f64 / config.frames as f64 / persistent.total_tiles as f64;
        println!(
            "{:>9.1}% {:>12.3} {:>13.3} {:>18.3} {:>12.3} {:>10.3} {:>10.2}",
            changed_ratio * 100.0,
            median_ms(&persistent.wall),
            median_ms(&persistent_full.wall),
            median_ms(&non_retained.wall),
            ms(persistent.materialize / config.frames as u32),
            ms(persistent.prepare / config.frames as u32),
            persistent.gpu_uploaded_bytes as f64 / config.frames as f64 / 1024.0,
        );
    }
    Ok(())
}

fn parse_config() -> Result<BenchConfig, Box<dyn Error>> {
    let mut config = BenchConfig::default();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--warmup" => config.warmup = value.parse()?,
            "--frames" => config.frames = value.parse()?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
        index += 2;
    }
    if config.frames == 0 {
        return Err("frames must be positive".into());
    }
    Ok(config)
}
