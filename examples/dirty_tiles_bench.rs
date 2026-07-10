use std::{
    error::Error,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use peniko::{
    Color,
    kurbo::{Affine, Point, Rect},
};
use tileink::{
    BlurSampling, Canvas, Filter, IncrementalOutputMode, IncrementalRenderMode, Mask, MaskKind,
    Radius, RectLiquidGlass, Region, RetainedNodeId, TextContext, TextFontSystem,
    TextLayoutOptions, WgpuRenderProfileReport, WgpuRenderer,
};

#[derive(Clone, Copy)]
struct Config {
    width: u32,
    height: u32,
    warmup: usize,
    frames: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            warmup: 8,
            frames: 80,
        }
    }
}

struct Scenario {
    name: &'static str,
    frames: Vec<Canvas>,
}

#[derive(Debug)]
struct Timing {
    average: f64,
    p50: f64,
    p95: f64,
    dirty_tiles: f64,
    total_tiles: u32,
    scanned_paths: f64,
    draw_batches: f64,
    root_draw_batches: f64,
    filter_dispatches: f64,
    compact_filter_dispatches: f64,
    reused_plan_ratio: f64,
    direct_output_ratio: f64,
    history_copy_ratio: f64,
}

struct WorkMetrics {
    dirty_tiles: f64,
    total_tiles: u32,
    scanned_paths: f64,
    draw_batches: f64,
    root_draw_batches: f64,
    filter_dispatches: f64,
    compact_filter_dispatches: f64,
    reused_plan_ratio: f64,
    direct_output_ratio: f64,
    history_copy_ratio: f64,
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = parse_config()?;
    let mut font_system = TextFontSystem::new();
    let mut text_context = TextContext::new();
    let scenarios = scenarios(config, &mut font_system, &mut text_context);
    let seed = WgpuRenderer::new_default_device(config.width, config.height, Color::TRANSPARENT);

    println!(
        "dirty tile bench: {}x{}, warmup {}, measured frames {}",
        config.width, config.height, config.warmup, config.frames
    );
    println!("timing includes CPU submit, GPU completion, and any required presentation copy\n");

    for scenario in scenarios {
        verify_parity(&seed, config, &scenario)?;
        let (auto, auto_profile) =
            bench_mode(&seed, config, &scenario, IncrementalRenderMode::Auto)?;
        let (full, full_profile) =
            bench_mode(&seed, config, &scenario, IncrementalRenderMode::ForceFull)?;
        let speedup = full.average / auto.average;
        println!(
            "{}: auto {:>7.3} ms avg (p50 {:>7.3}, p95 {:>7.3}), full {:>7.3} ms, {:>5.2}x, dirty {:.1}/{}, batches {:.1}/{:.1} root, scanned paths {:.1}, filter dispatches {:.1} ({:.1} compact), plan reuse {:.0}%, direct {:.0}%, history copy {:.0}%",
            scenario.name,
            auto.average,
            auto.p50,
            auto.p95,
            full.average,
            speedup,
            auto.dirty_tiles,
            auto.total_tiles,
            auto.root_draw_batches,
            auto.draw_batches,
            auto.scanned_paths,
            auto.filter_dispatches,
            auto.compact_filter_dispatches,
            auto.reused_plan_ratio * 100.0,
            auto.direct_output_ratio * 100.0,
            auto.history_copy_ratio * 100.0,
        );
        println!("auto stages\n{auto_profile}");
        println!("full stages\n{full_profile}\n");
    }
    Ok(())
}

fn parse_config() -> Result<Config, Box<dyn Error>> {
    let mut config = Config::default();
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--width" => config.width = value.parse()?,
            "--height" => config.height = value.parse()?,
            "--warmup" => config.warmup = value.parse()?,
            "--frames" => config.frames = value.parse()?,
            _ => return Err(format!("unknown argument {flag}").into()),
        }
        index += 2;
    }
    if config.width == 0 || config.height == 0 || config.frames == 0 {
        return Err("width, height, and frames must be positive".into());
    }
    Ok(config)
}

fn scenarios(
    config: Config,
    font_system: &mut TextFontSystem,
    text_context: &mut TextContext,
) -> Vec<Scenario> {
    vec![
        Scenario {
            name: "static-noop",
            frames: static_frames(config),
        },
        Scenario {
            name: "small-color-change",
            frames: color_frames(config),
        },
        Scenario {
            name: "small-move",
            frames: movement_frames(config),
        },
        Scenario {
            name: "text-edit",
            frames: text_edit_frames(config, font_system, text_context),
        },
        Scenario {
            name: "scroll-clip",
            frames: scroll_clip_frames(config),
        },
        Scenario {
            name: "add-remove",
            frames: add_remove_frames(config),
        },
        Scenario {
            name: "scattered-components",
            frames: scattered_frames(config),
        },
        Scenario {
            name: "large-damage-direct-output",
            frames: large_damage_frames(config),
        },
        Scenario {
            name: "blurred-offscreen",
            frames: filter_frames(config),
        },
        Scenario {
            name: "color-filter",
            frames: color_filter_frames(config),
        },
        Scenario {
            name: "backdrop-filter",
            frames: backdrop_frames(config),
        },
        Scenario {
            name: "offscreen-mask",
            frames: mask_frames(config),
        },
        Scenario {
            name: "liquid-glass",
            frames: liquid_glass_frames(config),
        },
        Scenario {
            name: "liquid-glass-non-retained",
            frames: non_retained_liquid_glass_frames(config),
        },
    ]
}

fn retained_root(config: Config, owner: u64) -> Canvas {
    Canvas::new_retained(
        config.width,
        config.height,
        1.0,
        RetainedNodeId::for_owner(owner),
    )
}

fn rect_scene(size: (u32, u32), rect: Rect, color: Color) -> Arc<Canvas> {
    let mut scene = Canvas::new(size.0, size.1, 1.0);
    scene.push_rect(rect, Radius::all(3.0), color);
    Arc::new(scene)
}

fn static_frames(config: Config) -> Vec<Canvas> {
    let scene = rect_scene(
        (config.width, config.height),
        Rect::new(16.0, 16.0, 240.0, 140.0),
        Color::from_rgb8(45, 110, 220),
    );
    (0..2)
        .map(|_| {
            let mut frame = retained_root(config, 100);
            frame.append_retained_scene(
                RetainedNodeId::for_owner(101),
                0,
                scene.clone(),
                (0.0, 0.0),
            );
            frame
        })
        .collect()
}

fn color_frames(config: Config) -> Vec<Canvas> {
    [Color::from_rgb8(220, 50, 40), Color::from_rgb8(40, 210, 80)]
        .into_iter()
        .enumerate()
        .map(|(revision, color)| {
            let mut frame = retained_root(config, 110);
            frame.append_retained_scene(
                RetainedNodeId::for_owner(111),
                revision as u64,
                rect_scene((64, 64), Rect::new(4.0, 4.0, 60.0, 60.0), color),
                (96.0, 96.0),
            );
            frame
        })
        .collect()
}

fn movement_frames(config: Config) -> Vec<Canvas> {
    let scene = rect_scene(
        (64, 64),
        Rect::new(2.0, 2.0, 62.0, 62.0),
        Color::from_rgb8(240, 170, 30),
    );
    [(120.0, 120.0), (184.0, 136.0)]
        .into_iter()
        .map(|position| {
            let mut frame = retained_root(config, 120);
            frame.append_retained_scene(RetainedNodeId::for_owner(121), 0, scene.clone(), position);
            frame
        })
        .collect()
}

fn text_edit_frames(
    config: Config,
    font_system: &mut TextFontSystem,
    text_context: &mut TextContext,
) -> Vec<Canvas> {
    ["retained text", "retained text editing"]
        .into_iter()
        .enumerate()
        .map(|(revision, text)| {
            let layout = text_context.layout(font_system, TextLayoutOptions::new(text, 32.0));
            let mut text_scene = Canvas::new(420, 64, 1.0);
            text_scene.push_text_layout_as_path(
                text_context,
                font_system,
                &layout,
                Point::new(4.0, 42.0),
                Color::from_rgb8(238, 242, 250),
                Affine::IDENTITY,
                0.1,
            );
            let mut frame = retained_root(config, 122);
            frame.append_retained_scene(
                RetainedNodeId::for_owner(123),
                revision as u64,
                Arc::new(text_scene),
                (48.0, 48.0),
            );
            frame
        })
        .collect()
}

fn scroll_clip_frames(config: Config) -> Vec<Canvas> {
    let mut content = Canvas::new(640, 240, 1.0);
    for index in 0..20 {
        let x = f64::from(index * 32);
        content.push_rect(
            Rect::new(x, 0.0, x + 24.0, 220.0),
            Radius::all(4.0),
            if index % 2 == 0 {
                Color::from_rgb8(40, 130, 220)
            } else {
                Color::from_rgb8(230, 120, 40)
            },
        );
    }
    let content = Arc::new(content);
    [0.0, -24.0]
        .into_iter()
        .map(|scroll_x| {
            let mut frame = retained_root(config, 124);
            let token = frame.begin_retained_node(RetainedNodeId::for_owner(125), 0);
            frame.push_clip_sdf_rect_layer(
                Rect::new(
                    48.0,
                    48.0,
                    f64::from(config.width.saturating_sub(48).max(49)),
                    f64::from(config.height.min(288).saturating_sub(32).max(49)),
                ),
                Radius::all(10.0),
            );
            frame.append_retained_scene(
                RetainedNodeId::for_owner(126),
                0,
                content.clone(),
                (48.0 + scroll_x, 48.0),
            );
            frame.pop_layer();
            frame.end_retained_node(token);
            frame
        })
        .collect()
}

fn add_remove_frames(config: Config) -> Vec<Canvas> {
    let stable = rect_scene(
        (64, 64),
        Rect::new(2.0, 2.0, 62.0, 62.0),
        Color::from_rgb8(70, 140, 220),
    );
    let transient = rect_scene(
        (48, 48),
        Rect::new(2.0, 2.0, 46.0, 46.0),
        Color::from_rgb8(240, 90, 120),
    );
    (0..2)
        .map(|phase| {
            let mut frame = retained_root(config, 127);
            frame.append_retained_scene(
                RetainedNodeId::for_owner(128),
                0,
                stable.clone(),
                (72.0, 72.0),
            );
            if phase == 0 {
                frame.append_retained_scene(
                    RetainedNodeId::for_owner(129),
                    0,
                    transient.clone(),
                    (176.0, 88.0),
                );
            }
            frame
        })
        .collect()
}

fn scattered_frames(config: Config) -> Vec<Canvas> {
    (0..2)
        .map(|phase| {
            let mut frame = retained_root(config, 130);
            for index in 0..256u64 {
                let x = ((index * 73) % config.width.saturating_sub(24).max(1) as u64) as f64;
                let y = ((index * 47) % config.height.saturating_sub(24).max(1) as u64) as f64;
                let changed = index % 64 == 0;
                let revision = u64::from(changed) * phase;
                let color = if changed && phase == 1 {
                    Color::from_rgb8(250, 80, 120)
                } else {
                    Color::from_rgb8(60, 130, 210)
                };
                frame.append_retained_scene(
                    RetainedNodeId::for_owner(1_000 + index),
                    revision,
                    rect_scene((24, 24), Rect::new(1.0, 1.0, 23.0, 23.0), color),
                    (x, y),
                );
            }
            frame
        })
        .collect()
}

fn large_damage_frames(config: Config) -> Vec<Canvas> {
    [
        Color::from_rgb8(35, 105, 220),
        Color::from_rgb8(220, 85, 55),
    ]
    .into_iter()
    .enumerate()
    .map(|(revision, color)| {
        let height = (config.height as f64 * 0.8).ceil();
        let scene = rect_scene(
            (config.width, config.height),
            Rect::new(0.0, 0.0, config.width as f64, height),
            color,
        );
        let mut frame = retained_root(config, 19);
        frame.append_retained_scene(
            RetainedNodeId::for_owner(190),
            revision as u64,
            scene,
            (0.0, 0.0),
        );
        frame
    })
    .collect()
}

fn filter_frames(config: Config) -> Vec<Canvas> {
    [Color::from_rgb8(220, 50, 50), Color::from_rgb8(40, 210, 90)]
        .into_iter()
        .enumerate()
        .map(|(revision, color)| {
            let mut frame = retained_root(config, 140);
            let token = frame.begin_retained_node(RetainedNodeId::for_owner(141), 0);
            frame.push_filter_layer(
                Filter::Blur {
                    std_dev_x: 8.0,
                    std_dev_y: 8.0,
                    sampling: BlurSampling::default(),
                },
                Region::rect(Rect::new(40.0, 40.0, 600.0, 400.0), Radius::all(12.0)),
            );
            frame.append_retained_scene(
                RetainedNodeId::for_owner(142),
                revision as u64,
                rect_scene((80, 80), Rect::new(4.0, 4.0, 76.0, 76.0), color),
                (160.0, 160.0),
            );
            frame.pop_layer();
            frame.end_retained_node(token);
            frame
        })
        .collect()
}

fn color_filter_frames(config: Config) -> Vec<Canvas> {
    [
        Color::from_rgb8(190, 80, 50),
        Color::from_rgb8(40, 150, 210),
    ]
    .into_iter()
    .enumerate()
    .map(|(revision, color)| {
        let mut frame = retained_root(config, 145);
        let token = frame.begin_retained_node(RetainedNodeId::for_owner(146), 0);
        frame.push_filter_layer(
            Filter::Brightness(1.2),
            Region::rect(Rect::new(40.0, 40.0, 600.0, 400.0), Radius::all(12.0)),
        );
        frame.append_retained_scene(
            RetainedNodeId::for_owner(147),
            revision as u64,
            rect_scene((64, 64), Rect::new(2.0, 2.0, 62.0, 62.0), color),
            (180.0, 120.0),
        );
        frame.pop_layer();
        frame.end_retained_node(token);
        frame
    })
    .collect()
}

fn backdrop_frames(config: Config) -> Vec<Canvas> {
    [Color::from_rgb8(30, 90, 220), Color::from_rgb8(220, 90, 40)]
        .into_iter()
        .enumerate()
        .map(|(revision, color)| {
            let mut frame = retained_root(config, 150);
            frame.append_retained_scene(
                RetainedNodeId::for_owner(151),
                revision as u64,
                rect_scene((80, 80), Rect::new(0.0, 0.0, 80.0, 80.0), color),
                (180.0, 140.0),
            );
            let token = frame.begin_retained_node(RetainedNodeId::for_owner(152), 0);
            frame.push_backdrop_layer(
                Filter::Blur {
                    std_dev_x: 6.0,
                    std_dev_y: 6.0,
                    sampling: BlurSampling::default(),
                },
                Region::rect(Rect::new(120.0, 100.0, 420.0, 320.0), Radius::all(18.0)),
            );
            frame.pop_layer();
            frame.end_retained_node(token);
            frame
        })
        .collect()
}

fn mask_frames(config: Config) -> Vec<Canvas> {
    [
        Color::from_rgb8(230, 60, 90),
        Color::from_rgb8(40, 200, 150),
    ]
    .into_iter()
    .enumerate()
    .map(|(revision, color)| {
        let mut frame = retained_root(config, 160);
        let mut mask = Canvas::new(config.width, config.height, 1.0);
        mask.push_rect(
            Rect::new(100.0, 100.0, 360.0, 300.0),
            Radius::all(48.0),
            Color::WHITE,
        );
        let token = frame.begin_retained_node(RetainedNodeId::for_owner(161), 0);
        frame.push_mask_layer(
            mask,
            Mask {
                region: Region::rect(Rect::new(100.0, 100.0, 360.0, 300.0), Radius::all(48.0)),
                kind: MaskKind::Alpha,
            },
        );
        frame.append_retained_scene(
            RetainedNodeId::for_owner(162),
            revision as u64,
            rect_scene((64, 64), Rect::new(0.0, 0.0, 64.0, 64.0), color),
            (140.0, 140.0),
        );
        frame.pop_layer();
        frame.end_retained_node(token);
        frame
    })
    .collect()
}

fn liquid_glass_frames(config: Config) -> Vec<Canvas> {
    [
        Color::from_rgb8(30, 100, 230),
        Color::from_rgb8(230, 80, 80),
    ]
    .into_iter()
    .enumerate()
    .map(|(revision, color)| {
        let mut frame = retained_root(config, 170);
        frame.append_retained_scene(
            RetainedNodeId::for_owner(171),
            revision as u64,
            rect_scene((72, 72), Rect::new(0.0, 0.0, 72.0, 72.0), color),
            (180.0, 120.0),
        );
        let token = frame.begin_retained_node(RetainedNodeId::for_owner(172), 0);
        frame.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 4,
                ..RectLiquidGlass::default()
            }),
            Region::rect(Rect::new(100.0, 80.0, 520.0, 340.0), Radius::all(24.0)),
        );
        frame.pop_layer();
        frame.end_retained_node(token);
        frame
    })
    .collect()
}

fn non_retained_liquid_glass_frames(config: Config) -> Vec<Canvas> {
    [
        Color::from_rgb8(30, 100, 230),
        Color::from_rgb8(230, 80, 80),
    ]
    .into_iter()
    .map(|color| {
        let mut frame = Canvas::new(config.width, config.height, 1.0);
        let marker = rect_scene((72, 72), Rect::new(0.0, 0.0, 72.0, 72.0), color);
        frame.append(&marker, (180.0, 120.0));
        frame.push_backdrop_layer(
            Filter::RectLiquidGlass(RectLiquidGlass {
                blur_radius: 4,
                ..RectLiquidGlass::default()
            }),
            Region::rect(Rect::new(100.0, 80.0, 520.0, 340.0), Radius::all(24.0)),
        );
        frame.pop_layer();
        frame
    })
    .collect()
}

fn bench_mode(
    seed: &WgpuRenderer,
    config: Config,
    scenario: &Scenario,
    mode: IncrementalRenderMode,
) -> Result<(Timing, WgpuRenderProfileReport), Box<dyn Error>> {
    let mut renderer = WgpuRenderer::new(
        seed.device(),
        seed.queue(),
        config.width,
        config.height,
        Color::TRANSPARENT,
    );
    let mut render_config = renderer.incremental_render_config();
    render_config.mode = mode;
    renderer.set_incremental_render_config(render_config);
    let texture = output_texture(renderer.device(), config.width, config.height);
    for index in 0..config.warmup {
        renderer
            .render_to_wgpu_texture(&scenario.frames[index % scenario.frames.len()], &texture)?;
        wait_for_gpu(renderer.device(), renderer.queue())?;
    }

    let mut samples = Vec::with_capacity(config.frames);
    let mut dirty = 0u64;
    let mut scanned_paths = 0u64;
    let mut draw_batches = 0u64;
    let mut root_draw_batches = 0u64;
    let mut filter_dispatches = 0u64;
    let mut compact_filter_dispatches = 0u64;
    let mut reused_plans = 0u64;
    let mut direct_outputs = 0u64;
    let mut history_copies = 0u64;
    let mut profile = WgpuRenderProfileReport::new();
    for index in 0..config.frames {
        let scene = &scenario.frames[index % scenario.frames.len()];
        renderer.start_profile();
        let start = Instant::now();
        renderer.render_to_wgpu_texture(scene, &texture)?;
        let _ = renderer.end_profile();
        wait_for_gpu(renderer.device(), renderer.queue())?;
        samples.push(start.elapsed());
        let resolved = renderer.poll_profile().clone();
        profile.push(&resolved);
        let stats = renderer.incremental_render_stats();
        dirty += stats.dirty_tiles as u64;
        scanned_paths += stats.scanned_paths as u64;
        draw_batches += stats.draw_batches as u64;
        root_draw_batches += stats.root_draw_batches as u64;
        filter_dispatches += stats.filter_dispatches as u64;
        compact_filter_dispatches += stats.compact_filter_dispatches as u64;
        reused_plans += u64::from(stats.reused_compiled_plan);
        direct_outputs += u64::from(stats.output_mode == IncrementalOutputMode::DirectTransient);
        history_copies += u64::from(stats.history_copied_to_output);
    }
    Ok((
        timing(
            &samples,
            WorkMetrics {
                dirty_tiles: dirty as f64 / config.frames as f64,
                total_tiles: renderer.incremental_render_stats().total_tiles,
                scanned_paths: scanned_paths as f64 / config.frames as f64,
                draw_batches: draw_batches as f64 / config.frames as f64,
                root_draw_batches: root_draw_batches as f64 / config.frames as f64,
                filter_dispatches: filter_dispatches as f64 / config.frames as f64,
                compact_filter_dispatches: compact_filter_dispatches as f64 / config.frames as f64,
                reused_plan_ratio: reused_plans as f64 / config.frames as f64,
                direct_output_ratio: direct_outputs as f64 / config.frames as f64,
                history_copy_ratio: history_copies as f64 / config.frames as f64,
            },
        ),
        profile,
    ))
}

fn verify_parity(
    seed: &WgpuRenderer,
    config: Config,
    scenario: &Scenario,
) -> Result<(), Box<dyn Error>> {
    let render = |mode| -> Result<Vec<u8>, Box<dyn Error>> {
        let mut renderer = WgpuRenderer::new(
            seed.device(),
            seed.queue(),
            config.width,
            config.height,
            Color::TRANSPARENT,
        );
        let mut renderer_config = renderer.incremental_render_config();
        renderer_config.mode = mode;
        renderer.set_incremental_render_config(renderer_config);
        let texture = output_texture(renderer.device(), config.width, config.height);
        let mut checkpoints = Vec::new();
        for (index, frame) in scenario.frames.iter().enumerate() {
            renderer.render_to_wgpu_texture(frame, &texture)?;
            if index == 0 || index + 1 == scenario.frames.len() {
                checkpoints.extend(read_output_texture(
                    renderer.device(),
                    renderer.queue(),
                    &texture,
                    config.width,
                    config.height,
                )?);
            }
        }
        Ok(checkpoints)
    };
    let auto = render(IncrementalRenderMode::Auto)?;
    let full = render(IncrementalRenderMode::ForceFull)?;
    if auto != full {
        let byte = auto
            .iter()
            .zip(&full)
            .position(|(auto, full)| auto != full)
            .unwrap_or(0);
        let pixels_per_checkpoint = config.width as usize * config.height as usize;
        let pixel = byte / 4;
        let checkpoint = pixel / pixels_per_checkpoint;
        let local_pixel = pixel % pixels_per_checkpoint;
        let x = local_pixel as u32 % config.width;
        let y = local_pixel as u32 / config.width;
        return Err(format!(
            "{} incremental output differs from full render at checkpoint {checkpoint}, ({x}, {y}): auto={}, full={}",
            scenario.name, auto[byte], full[byte]
        )
        .into());
    }
    Ok(())
}

fn timing(samples: &[Duration], work: WorkMetrics) -> Timing {
    let mut milliseconds = samples
        .iter()
        .map(|sample| sample.as_secs_f64() * 1_000.0)
        .collect::<Vec<_>>();
    milliseconds.sort_by(f64::total_cmp);
    let percentile =
        |value: f64| milliseconds[((milliseconds.len() - 1) as f64 * value).round() as usize];
    Timing {
        average: milliseconds.iter().sum::<f64>() / milliseconds.len() as f64,
        p50: percentile(0.50),
        p95: percentile(0.95),
        dirty_tiles: work.dirty_tiles,
        total_tiles: work.total_tiles,
        scanned_paths: work.scanned_paths,
        draw_batches: work.draw_batches,
        root_draw_batches: work.root_draw_batches,
        filter_dispatches: work.filter_dispatches,
        compact_filter_dispatches: work.compact_filter_dispatches,
        reused_plan_ratio: work.reused_plan_ratio,
        direct_output_ratio: work.direct_output_ratio,
        history_copy_ratio: work.history_copy_ratio,
    }
}

fn output_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tileink dirty tile benchmark output"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn read_output_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let row_bytes = width as u64 * 4;
    let padded_row_bytes = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tileink dirty tile benchmark parity readback"),
        size: padded_row_bytes * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("tileink dirty tile benchmark parity copy"),
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row_bytes as u32),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;

    let mapped = buffer.slice(..).get_mapped_range()?;
    let mut pixels = Vec::with_capacity((row_bytes * height as u64) as usize);
    for row in 0..height as usize {
        let start = row * padded_row_bytes as usize;
        pixels.extend_from_slice(&mapped[start..start + row_bytes as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}

fn wait_for_gpu(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), Box<dyn Error>> {
    let (tx, rx) = mpsc::channel();
    queue.on_submitted_work_done(move || {
        let _ = tx.send(());
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()?;
    Ok(())
}
