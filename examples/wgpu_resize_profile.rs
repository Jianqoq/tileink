//! Diagnostic frame distributions for the same resize cycle as Criterion.
//! Timestamp/readback instrumentation is separate from the production benchmark.

#[path = "common/resize.rs"]
mod resize;

use peniko::Color;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fmt::Write as _, path::PathBuf, time::Instant};
use tileink::WgpuRenderer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: wgpu_resize_profile vulkan|dx12 NEW_OUTPUT_JSON".into());
    }
    let api = args[0].to_str().ok_or("API is not UTF-8")?;
    // Reserve the output before doing expensive work; never overwrite a previous run.
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(PathBuf::from(&args[1]))?;
    let mut routes = Vec::new();
    for portable in [false, true] {
        let (adapter, device, queue) =
            resize::device(api, portable, true, wgpu::MemoryHints::Performance);
        let mut scene = resize::Scene::new();
        let mut renderer = WgpuRenderer::new(
            &device,
            &queue,
            scene.size.0,
            scene.size.1,
            Color::TRANSPARENT,
        );
        let mut full = resize::full_renderer(&device, &queue);
        scene.verify_cycle(&mut renderer, &mut full);
        let pipelines = renderer.pipeline_compilation_epoch();
        assert_eq!(
            renderer.precompiled_dxil_pipeline_count(),
            0,
            "resize profile must use runtime shaders"
        );
        let mut frames: Vec<Value> = Vec::new();
        // Production distribution: no timestamp queries, resolves, copies or profile maps.
        for frame in 0..resize::CYCLE * 4 {
            let start = Instant::now();
            scene.advance();
            let scene_done = start.elapsed();
            renderer.render_retained(&scene.retained);
            let rendered = start.elapsed();
            device.poll(wgpu::PollType::wait_indefinitely())?;
            let complete = start.elapsed();
            frames.push(json!({
                "frame":frame, "width":scene.size.0, "height":scene.size.1,
                "scene_update_us":scene_done.as_secs_f64()*1e6,
                "render_record_submit_us":(rendered-scene_done).as_secs_f64()*1e6,
                "gpu_wait_us":(complete-rendered).as_secs_f64()*1e6,
                "complete_us":complete.as_secs_f64()*1e6,
            }));
        }
        scene.verify(&mut renderer, &mut full);
        // A separate pass provides diagnostic stage timings for the same ordered sizes.
        // These stage times must never be substituted for a production PMax frame.
        let mut diagnostics = Vec::new();
        for frame in 0..resize::CYCLE * 4 {
            renderer.start_profile();
            scene.advance();
            renderer.render_retained(&scene.retained);
            renderer.end_profile();
            device.poll(wgpu::PollType::wait_indefinitely())?;
            renderer.poll_profile();
            while renderer.has_pending_profile_readbacks() {
                device.poll(wgpu::PollType::wait_indefinitely())?;
                renderer.poll_profile();
            }
            let profile = renderer.profile();
            assert!(
                profile
                    .summary()
                    .iter()
                    .any(|entry| entry.gpu_duration.is_some())
            );
            diagnostics.push(json!({
                "frame":frame, "width":scene.size.0, "height":scene.size.1,
                "stages":profile.summary().iter().map(|entry|json!({
                    "name":entry.name,
                    "cpu_us":entry.cpu_duration.map(|time|time.as_secs_f64()*1e6),
                    "gpu_us":entry.gpu_duration.map(|time|time.as_secs_f64()*1e6),
                })).collect::<Vec<_>>(),
            }));
        }
        assert_eq!(renderer.pipeline_compilation_epoch(), pipelines);
        scene.verify(&mut renderer, &mut full);
        let mut order: Vec<_> = (0..frames.len()).collect();
        order.sort_by(|&left, &right| {
            frames[left]["complete_us"]
                .as_f64()
                .unwrap()
                .total_cmp(&frames[right]["complete_us"].as_f64().unwrap())
        });
        let rank = |percent: usize| order[(frames.len() * percent).div_ceil(100) - 1];
        let max_frame = *order.last().unwrap();
        routes.push(json!({
            "adapter":adapter,
            "texture_mode":if portable { "portable" } else { "native" },
            "sample_count":frames.len(), "pipeline_count":pipelines,
            "precompiled_dxil_pipelines":renderer.precompiled_dxil_pipeline_count(),
            "p50_us":frames[rank(50)]["complete_us"],
            "p95_us":frames[rank(95)]["complete_us"],
            "pmax_us":frames[max_frame]["complete_us"],
            "pmax_frame":frames[max_frame],
            "pmax_time_shares_pct":{
                "scene_update":100.0*frames[max_frame]["scene_update_us"].as_f64().unwrap()/frames[max_frame]["complete_us"].as_f64().unwrap(),
                "render_record_submit":100.0*frames[max_frame]["render_record_submit_us"].as_f64().unwrap()/frames[max_frame]["complete_us"].as_f64().unwrap(),
                "gpu_wait":100.0*frames[max_frame]["gpu_wait_us"].as_f64().unwrap()/frames[max_frame]["complete_us"].as_f64().unwrap(),
            },
            "frames":frames,
            "diagnostic_pass":diagnostics, "verified_initial_frame":true, "verified_cycle_frames":resize::CYCLE,
        }));
        println!("Profiled {api} resize: portable={portable}, 256 frames");
    }
    let binary = std::env::current_exe()?;
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(std::fs::read(&binary)?) {
        write!(hash, "{byte:02x}")?;
    }
    serde_json::to_writer_pretty(
        output,
        &json!({
            "schema":1, "api":api, "binary":binary, "binary_sha256":hash,
            "scenario":"owned retained target, 961x601 to 1217x761, 64-frame grow/shrink cycle",
            "production_distribution_uses_profiler":false, "separate_diagnostic_pass":true,
            "timing_contract":"production frames include scene update + render/record/submit + GPU completion, with no profiler or readback; diagnostics are a separate ordered pass and not the production PMax stages; no window/swapchain",
            "routes":routes,
        }),
    )?;
    Ok(())
}
