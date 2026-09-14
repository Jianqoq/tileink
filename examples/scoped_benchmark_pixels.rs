//! Correctness prerequisite for the unchanged scoped CPU benchmark.
//! This executable performs readbacks separately from all Criterion timings.
#[path = "support/retained_bench.rs"]
mod retained_bench;
use retained_bench::benchmark_gpu;

#[allow(dead_code)]
mod workload {
    include!("../benches/scoped_damage.rs");

    pub fn make_scene(count: usize, scoped: bool) -> RetainedScene {
        scene(count, scoped)
    }

    pub fn toggle(scene: &mut RetainedScene, replace_leaf: bool, changed: bool) {
        let mut tx = scene.transaction();
        if replace_leaf {
            tx.replace_scene(
                LEAF,
                leaf(if changed {
                    Color::from_rgb8(255, 0, 0)
                } else {
                    Color::from_rgb8(0, 0, 255)
                }),
            );
        } else {
            tx.update_layer(SOURCE, source_layer(if changed { 1.0 } else { 0.0 }));
        }
        tx.commit().unwrap();
    }
}

use peniko::Color;
use peniko::kurbo::Rect;
use sha2::{Digest, Sha256};
use tileink::{Canvas, Filter, IncrementalRenderMode, Radius, Region};

// Build the same visual scene directly with public immediate-mode commands.
// RetainedScene::to_canvas is private/test-only, so this independent oracle
// deliberately avoids both the persistent materializer and private test APIs.
fn flat_scene(count: usize, scoped: bool, replace_leaf: bool, changed: bool) -> Canvas {
    let region = |rect| Region::rect(rect, Radius::ZERO);
    let mut canvas = Canvas::new(1024, 1024, 1.0);
    if scoped {
        canvas.push_filter_layer(
            Filter::Offset { dx: 16.0, dy: 0.0 },
            region(Rect::new(0.0, 0.0, 1024.0, 1024.0)),
        );
    }
    canvas.push_filter_layer(
        Filter::Invert(if changed && !replace_leaf { 1.0 } else { 0.0 }),
        region(Rect::new(0.0, 0.0, 8.0, 8.0)),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 8.0, 8.0),
        Radius::ZERO,
        if changed && replace_leaf {
            Color::from_rgb8(255, 0, 0)
        } else {
            Color::from_rgb8(0, 0, 255)
        },
    );
    canvas.pop_layer();
    for index in 0..count {
        let x = (index % 64) as f64 * 16.0;
        let y = (index / 64) as f64 * 16.0;
        canvas.push_backdrop_layer(
            Filter::Invert(1.0),
            region(Rect::new(x, y, x + 8.0, y + 8.0)),
        );
        canvas.pop_layer();
    }
    if scoped {
        canvas.pop_layer();
    }
    canvas
}

fn main() {
    let api = std::env::var("TILEINK_BENCH_API").expect("explicit API required");
    let mode = std::env::var("TILEINK_WGPU_MODE").expect("explicit texture mode required");
    assert!(matches!(mode.as_str(), "native" | "portable"));
    let (_, device, queue) = benchmark_gpu::device(
        &api,
        mode == "portable",
        false,
        wgpu::MemoryHints::Performance,
    );
    let context = retained_bench::BenchContext::new(&device, &queue);
    for (group, scoped, replace_leaf, counts) in [
        ("root-layer-parameter", false, false, &[0, 1, 100, 1000][..]),
        ("scoped-layer-parameter", true, false, &[1, 100, 1000][..]),
        ("scoped-leaf-revision", true, true, &[1, 100, 1000][..]),
    ] {
        for &count in counts {
            let mut scene = workload::make_scene(count, scoped);
            let mut incremental = context.renderer();
            let mut full = context.renderer();
            let mut config = full.incremental_render_config();
            config.mode = IncrementalRenderMode::ForceFull;
            full.set_incremental_render_config(config);
            let mut previous = None;
            for step in 0..3 {
                if step > 0 {
                    let version = scene.version();
                    workload::toggle(&mut scene, replace_leaf, step == 1);
                    assert_ne!(scene.version(), version, "transaction did not advance");
                }
                incremental.render_retained(&scene);
                full.render_retained(&scene);
                let actual = incremental.image();
                assert!(
                    actual.pixels == full.image().pixels,
                    "incremental/ForceFull mismatch: {group}/{count} step {step}"
                );
                // Fresh immediate-mode commands provide an independent visual oracle.
                let mut flat = context.renderer();
                flat.render(&flat_scene(count, scoped, replace_leaf, step == 1));
                assert!(
                    actual.pixels == flat.image().pixels,
                    "incremental/flattened mismatch: {group}/{count} step {step}"
                );
                assert_eq!(actual.pixels.len(), 1024 * 1024);
                let mut expected = if step == 1 {
                    if replace_leaf {
                        [255, 0, 0, 255]
                    } else {
                        [255, 255, 0, 255]
                    }
                } else {
                    [0, 0, 255, 255]
                };
                if count > 0 {
                    for channel in &mut expected[..3] {
                        *channel = 255 - *channel;
                    }
                }
                assert_eq!(actual.rgba8_at(if scoped { 20 } else { 4 }, 4), expected);
                // Packed bytes include alpha and RGB under transparent pixels.
                // This executable is never used for Criterion timing.
                let bytes: Vec<u8> = actual
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_le_bytes())
                    .collect();
                let output = std::env::var("TILEINK_SCOPED_RGBA_OUTPUT")
                    .expect("explicit capture directory required");
                let path =
                    std::path::Path::new(&output).join(format!("{group}-{count}-{step}.rgba"));
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .unwrap();
                std::io::Write::write_all(&mut file, &bytes).unwrap();
                let digest: String = Sha256::digest(&bytes)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                if let Some(previous) = previous.replace(digest.clone()) {
                    assert_ne!(
                        digest, previous,
                        "a real toggle produced an unchanged frame"
                    );
                }
                println!(
                    "scoped pixels: {}",
                    serde_json::json!({
                        "case":format!("{group}/{count}"), "step":step, "width":1024, "height":1024,
                        "rgba_sha256":digest, "incremental_equals_force_full":true,
                        "incremental_equals_flattened":true, "analytic_probe":expected,
                    })
                );
            }
        }
    }
}
