#[path = "support/retained_bench.rs"]
mod retained_bench_support;

use std::{error::Error, sync::Arc};

use peniko::{Color, kurbo::Rect};
use retained_bench_support::{BenchConfig, HEIGHT, WIDTH, bench, median_ms};
use tileink::{Canvas, IncrementalRenderMode, Radius, RetainedNodeId, WgpuRenderer};

const BACKGROUND_NODES: usize = 4096;
const RATIOS: [f64; 13] = [
    0.005, 0.01, 0.02, 0.05, 0.10, 0.20, 0.30, 0.40, 0.50, 0.60, 0.70, 0.80, 1.00,
];

fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    let background = rect_scene(8, 8, Color::from_rgb8(40, 80, 140));
    println!(
        "retained dirty-ratio bench: {WIDTH}x{HEIGHT}, {BACKGROUND_NODES} unchanged background nodes, warmup {}, measured frames {}",
        config.warmup, config.frames
    );
    println!(
        "render timing excludes Canvas construction; preflat immediate is a lower bound, not an end-to-end comparison"
    );
    println!(
        "{:<10} {:>11} {:>13} {:>12} {:>18}",
        "changed", "auto ms", "force-full", "full/auto", "preflat immediate"
    );

    for requested_ratio in RATIOS {
        let retained = retained_frames(requested_ratio, &background);
        let immediate = immediate_frames(requested_ratio, &background);
        let auto = bench(&seed, config, &retained, IncrementalRenderMode::Auto)?;
        let retained_full = bench(&seed, config, &retained, IncrementalRenderMode::ForceFull)?;
        let non_retained = bench(&seed, config, &immediate, IncrementalRenderMode::Auto)?;
        let auto_ms = median_ms(&auto.wall);
        let retained_full_ms = median_ms(&retained_full.wall);
        let non_retained_ms = median_ms(&non_retained.wall);
        let changed_ratio =
            auto.changed_tiles as f64 / config.frames as f64 / auto.total_tiles as f64;
        println!(
            "{:>9.1}% {:>11.3} {:>13.3} {:>11.2}x {:>18.3}",
            changed_ratio * 100.0,
            auto_ms,
            retained_full_ms,
            retained_full_ms / auto_ms,
            non_retained_ms,
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

fn retained_frames(ratio: f64, background: &Arc<Canvas>) -> [Canvas; 2] {
    let side = (ratio.sqrt() * WIDTH as f64).clamp(1.0, WIDTH as f64);
    let first = rect_scene(WIDTH, HEIGHT, Color::from_rgb8(230, 80, 40));
    let second = rect_scene(WIDTH, HEIGHT, Color::from_rgb8(40, 210, 90));
    [
        retained_frame(side, 0, background, first),
        retained_frame(side, 1, background, second),
    ]
}

fn retained_frame(
    damage_side: f64,
    revision: u64,
    background: &Arc<Canvas>,
    changing: Arc<Canvas>,
) -> Canvas {
    let mut frame = Canvas::new_retained(WIDTH, HEIGHT, 1.0, RetainedNodeId::for_owner(1));
    for index in 0..BACKGROUND_NODES {
        frame.append_retained_scene(
            RetainedNodeId::for_owner(index as u64 + 2),
            0,
            background.clone(),
            ((index % 64) as f64 * 16.0, (index / 64) as f64 * 16.0),
        );
    }
    frame.append_retained_scene(
        RetainedNodeId::for_owner(BACKGROUND_NODES as u64 + 2),
        revision,
        clipped_scene(changing, damage_side),
        (0.0, 0.0),
    );
    frame
}

fn immediate_frames(ratio: f64, background: &Arc<Canvas>) -> [Canvas; 2] {
    let side = (ratio.sqrt() * WIDTH as f64).clamp(1.0, WIDTH as f64);
    [
        immediate_frame(
            background,
            clipped_scene(
                rect_scene(WIDTH, HEIGHT, Color::from_rgb8(230, 80, 40)),
                side,
            ),
        ),
        immediate_frame(
            background,
            clipped_scene(
                rect_scene(WIDTH, HEIGHT, Color::from_rgb8(40, 210, 90)),
                side,
            ),
        ),
    ]
}

fn immediate_frame(background: &Arc<Canvas>, changing: Arc<Canvas>) -> Canvas {
    let mut frame = Canvas::new(WIDTH, HEIGHT, 1.0);
    for index in 0..BACKGROUND_NODES {
        frame.append(
            background,
            ((index % 64) as f64 * 16.0, (index / 64) as f64 * 16.0),
        );
    }
    frame.append(&changing, (0.0, 0.0));
    frame
}

fn clipped_scene(scene: Arc<Canvas>, damage_side: f64) -> Arc<Canvas> {
    let mut clipped = Canvas::new(WIDTH, HEIGHT, 1.0);
    clipped.push_clip_sdf_rect_layer(Rect::new(0.0, 0.0, damage_side, damage_side), Radius::ZERO);
    clipped.append(&scene, (0.0, 0.0));
    clipped.pop_layer();
    Arc::new(clipped)
}

fn rect_scene(width: u32, height: u32, color: Color) -> Arc<Canvas> {
    let mut scene = Canvas::new(width, height, 1.0);
    scene.push_rect(
        Rect::new(0.0, 0.0, width as f64, height as f64),
        Radius::ZERO,
        color,
    );
    Arc::new(scene)
}
