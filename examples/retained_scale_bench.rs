#[path = "support/retained_bench.rs"]
mod retained_bench;
#[path = "support/retained_scale.rs"]
mod retained_scale;

use std::error::Error;

use peniko::Color;
use retained_bench::{
    BenchConfig, HEIGHT, MutationPhase, WIDTH, bench_persistent, bench_persistent_phase, median_ms,
    ms,
};
use retained_scale::{Scenario, Workload};
use tileink::{IncrementalRenderMode, WgpuRenderer};

fn main() -> Result<(), Box<dyn Error>> {
    let RunConfig {
        bench: config,
        counts,
        scenarios,
        phase,
    } = parse_config()?;
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    println!(
        "persistent retained scale bench: {WIDTH}x{HEIGHT}, warmup {}, measured frames {}",
        config.warmup, config.frames
    );
    println!(
        "wall includes submit and GPU completion; transaction is reported separately from render"
    );
    println!(
        "{:<20} {:>7} {:>9} {:>10} {:>10} {:>10} {:>12} {:>10} {:>10} {:>9} {:>9} {:>9} {:>9} {:>9} {:>7} {:>7} {:>7} {:>5} {:>10} {:>10} {:>8} {:>7} {:>10} {:>10} {:>7} {:>7}",
        "scenario",
        "nodes",
        "txn ms",
        "wall ms",
        "cpu ms",
        "collect",
        "materialize",
        "damage",
        "prepare",
        "scan",
        "raster",
        "select",
        "execute",
        "dirty",
        "chunks",
        "plans",
        "batches",
        "sync",
        "CPU KiB",
        "GPU KiB",
        "pages",
        "pg cmp",
        "live KiB",
        "cap KiB",
        "frag %",
        "compact"
    );

    for count in counts {
        for &scenario in &scenarios {
            let workload = Workload::new(count, scenario);
            let scene = workload.build_scene();
            let measured = if let Some(phase) = phase {
                bench_persistent_phase(
                    &seed,
                    config,
                    scene,
                    IncrementalRenderMode::Auto,
                    phase,
                    |scene, frame| workload.mutate(scene, frame),
                )?
            } else {
                bench_persistent(
                    &seed,
                    config,
                    scene,
                    IncrementalRenderMode::Auto,
                    |scene, frame| workload.mutate(scene, frame),
                )?
            };
            let n = config.frames as u32;
            println!(
                "{:<20} {:>7} {:>9.3} {:>10.3} {:>10.3} {:>10.3} {:>12.3} {:>10.3} {:>10.3} {:>9.3} {:>9.3} {:>9.3} {:>9.3} {:>9.1} {:>7.1} {:>7.1} {:>7.1} {:>5} {:>10.2} {:>10.2} {:>8.1} {:>7} {:>10.1} {:>10.1} {:>7.1} {:>7}",
                scenario.name(),
                count,
                ms(measured.transaction / n),
                median_ms(&measured.wall),
                ms(measured.cpu / n),
                ms(measured.collect / n),
                ms(measured.materialize / n),
                ms(measured.damage / n),
                ms(measured.prepare / n),
                ms(measured.scan / n),
                ms(measured.raster / n),
                ms(measured.plan_select / n),
                ms(measured.plan_execute / n),
                measured.dirty_tiles as f64 / config.frames as f64,
                measured.chunks_rebuilt as f64 / config.frames as f64,
                measured.plan_fragments_rebuilt as f64 / config.frames as f64,
                measured.root_draw_batches as f64 / config.frames as f64,
                measured.full_scene_syncs,
                measured.cpu_copied_bytes as f64 / config.frames as f64 / 1024.0,
                measured.gpu_uploaded_bytes as f64 / config.frames as f64 / 1024.0,
                measured.tile_pages_rewritten as f64 / config.frames as f64,
                measured.tile_page_compactions,
                measured.arena_live_bytes as f64 / config.frames as f64 / 1024.0,
                measured.arena_capacity_bytes as f64 / config.frames as f64 / 1024.0,
                measured.arena_fragmentation / config.frames as f64 * 100.0,
                measured.arena_compactions,
            );
            if measured.materialize > std::time::Duration::ZERO {
                println!(
                    "  materialize detail: analysis {:.3} ms; chunks {:.3}; plan+sync {:.3}; frame {:.3} ms",
                    ms(measured.materialize_analysis / n),
                    ms(measured.materialize_chunks / n),
                    ms(measured.materialize_plan_sync / n),
                    ms(measured.materialize_frame / n),
                );
            }
            if measured.group_cache
                + measured.group_children
                + measured.group_mask
                + measured.group_composite
                > std::time::Duration::ZERO
            {
                println!(
                    "  plan detail: draw {:.3} ms; group cache {:.3}, children {:.3}, scratch {:.3}, render {:.3}, mask {:.3}, composite {:.3} ms",
                    ms(measured.draw_batch / n),
                    ms(measured.group_cache / n),
                    ms(measured.group_children / n),
                    ms(measured.group_scratch / n),
                    ms(measured.group_render / n),
                    ms(measured.group_mask / n),
                    ms(measured.group_composite / n),
                );
            }
        }
    }
    Ok(())
}

struct RunConfig {
    bench: BenchConfig,
    counts: Vec<usize>,
    scenarios: Vec<Scenario>,
    phase: Option<MutationPhase>,
}

fn parse_config() -> Result<RunConfig, Box<dyn Error>> {
    let mut config = BenchConfig::default();
    let mut counts = vec![100, 1_000, 5_000, 20_000, 100_000];
    let mut scenarios = Scenario::ALL.to_vec();
    let mut phase = None;
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
            "--counts" => {
                counts = value
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<Vec<_>, _>>()?
            }
            "--scenarios" => {
                scenarios = value
                    .split(',')
                    .map(|name| {
                        Scenario::parse(name)
                            .ok_or_else(|| format!("unknown benchmark scenario {name}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            }
            "--phase" => {
                phase = Some(match value.as_str() {
                    "insert" => MutationPhase::Insert,
                    "remove" => MutationPhase::Remove,
                    _ => return Err(format!("unknown mutation phase {value}").into()),
                });
            }
            _ => return Err(format!("unknown argument {flag}").into()),
        }
        index += 2;
    }
    if config.frames == 0 || counts.is_empty() || counts.contains(&0) || scenarios.is_empty() {
        return Err("frames, scenarios, and every node count must be positive".into());
    }
    Ok(RunConfig {
        bench: config,
        counts,
        scenarios,
        phase,
    })
}
