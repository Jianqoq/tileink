#[path = "support/retained_bench.rs"]
mod retained_bench_support;

use std::{error::Error, sync::Arc};

use peniko::{
    Color,
    kurbo::{Point, Rect},
};
use retained_bench_support::{BenchConfig, HEIGHT, WIDTH, bench, median_ms, ms};
use tileink::{Canvas, IncrementalRenderMode, Radius, RetainedNodeId, WgpuRenderer};

#[derive(Clone, Copy)]
enum Scenario {
    Static,
    OneRevision,
    AllRevisions,
    OneMove,
    AddRemove,
    Reorder,
    ManualInvalidation,
}

impl Scenario {
    const ALL: [Self; 7] = [
        Self::Static,
        Self::OneRevision,
        Self::AllRevisions,
        Self::OneMove,
        Self::AddRemove,
        Self::Reorder,
        Self::ManualInvalidation,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::OneRevision => "one-revision",
            Self::AllRevisions => "all-revisions",
            Self::OneMove => "one-move",
            Self::AddRemove => "add-remove",
            Self::Reorder => "reorder",
            Self::ManualInvalidation => "manual-invalidation",
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let (config, counts) = parse_config()?;
    let seed = WgpuRenderer::new_default_device(WIDTH, HEIGHT, Color::TRANSPARENT);
    println!(
        "retained scale bench: {WIDTH}x{HEIGHT}, warmup {}, measured frames {}",
        config.warmup, config.frames
    );
    println!(
        "wall includes CPU submit and GPU completion; stage columns are profiler CPU time per frame"
    );
    println!(
        "{:<20} {:>7} {:>10} {:>10} {:>10} {:>12} {:>10} {:>10} {:>9}",
        "scenario",
        "nodes",
        "wall ms",
        "cpu ms",
        "collect",
        "materialize",
        "damage",
        "prepare",
        "dirty"
    );

    for count in counts {
        let shared = rect_scene(Color::from_rgb8(30, 130, 220));
        let changed = rect_scene(Color::from_rgb8(230, 90, 40));
        for scenario in Scenario::ALL {
            let frames = build_frames(count, scenario, &shared, &changed);
            let measured = bench(&seed, config, &frames, IncrementalRenderMode::Auto)?;
            let n = config.frames as u32;
            println!(
                "{:<20} {:>7} {:>10.3} {:>10.3} {:>10.3} {:>12.3} {:>10.3} {:>10.3} {:>9.1}",
                scenario.name(),
                count,
                median_ms(&measured.wall),
                ms(measured.cpu / n),
                ms(measured.collect / n),
                ms(measured.materialize / n),
                ms(measured.damage / n),
                ms(measured.prepare / n),
                measured.dirty_tiles as f64 / config.frames as f64,
            );
        }
    }
    Ok(())
}

fn parse_config() -> Result<(BenchConfig, Vec<usize>), Box<dyn Error>> {
    let mut config = BenchConfig::default();
    let mut counts = vec![10, 100, 1_000, 5_000];
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
            _ => return Err(format!("unknown argument {flag}").into()),
        }
        index += 2;
    }
    if config.frames == 0 || counts.is_empty() || counts.contains(&0) {
        return Err("frames and every node count must be positive".into());
    }
    Ok((config, counts))
}

fn rect_scene(color: Color) -> Arc<Canvas> {
    let mut scene = Canvas::new(8, 8, 1.0);
    scene.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, color);
    Arc::new(scene)
}

fn build_frames(
    count: usize,
    scenario: Scenario,
    shared: &Arc<Canvas>,
    changed: &Arc<Canvas>,
) -> [Canvas; 2] {
    let mut first_order = (0..count).collect::<Vec<_>>();
    let mut second_order = first_order.clone();
    if matches!(scenario, Scenario::Reorder) && count > 1 {
        second_order.rotate_left(1);
    }
    let second_count = if matches!(scenario, Scenario::AddRemove) {
        count.saturating_sub(1)
    } else {
        count
    };
    first_order.truncate(count);
    second_order.truncate(second_count);

    [
        build_frame(count, scenario, 0, &first_order, shared, changed),
        build_frame(count, scenario, 1, &second_order, shared, changed),
    ]
}

fn build_frame(
    count: usize,
    scenario: Scenario,
    phase: usize,
    order: &[usize],
    shared: &Arc<Canvas>,
    changed: &Arc<Canvas>,
) -> Canvas {
    let mut frame = Canvas::new_retained(WIDTH, HEIGHT, 1.0, RetainedNodeId::for_owner(1));
    if matches!(scenario, Scenario::ManualInvalidation) {
        let color = if phase == 0 {
            Color::BLACK
        } else {
            Color::WHITE
        };
        frame.push_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Radius::ZERO, color);
        frame.invalidate_rect(Rect::new(0.0, 0.0, 8.0, 8.0));
    }
    for &index in order {
        let all_changed = matches!(scenario, Scenario::AllRevisions) && phase == 1;
        let one_changed = matches!(scenario, Scenario::OneRevision) && phase == 1 && index == 0;
        let revision = u64::from(all_changed || one_changed);
        let scene = if one_changed { changed } else { shared };
        let mut position = position(index, count);
        if matches!(scenario, Scenario::OneMove) && phase == 1 && index == 0 {
            position.x += 16.0;
        }
        frame.append_retained_scene(
            RetainedNodeId::for_owner(index as u64 + 2),
            revision,
            scene.clone(),
            position,
        );
    }
    frame
}

fn position(index: usize, count: usize) -> Point {
    let columns = (count as f64).sqrt().ceil().max(1.0) as usize;
    Point::new(
        (index % columns) as f64 * 10.0,
        (index / columns) as f64 * 10.0,
    )
}
