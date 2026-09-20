//! Full SVG corpus, exported and compared in separate mutually exclusive builds.
#![cfg(all(target_os = "macos", any(feature = "wgpu", feature = "metal")))]
#[cfg(feature = "wgpu")]
#[allow(dead_code)]
#[path = "../examples/common/benchmark_gpu.rs"]
mod gpu;
use peniko::{Color, kurbo::Affine};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tileink::{Canvas, SvgOptions};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn files(directory: &Path, extension: &str, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            files(&path, extension, output)?;
        } else if path.extension().is_some_and(|e| e == extension) {
            output.push(path);
        }
    }
    Ok(())
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
#[ignore = "full same-device SVG certification; run scripts/mac/run_native_metal_tests.sh --svg"]
fn full_svg_corpus_matches_same_device() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = root.join("target/metal-validation/svg");
    std::fs::create_dir_all(&directory)?;
    let mut inputs = Vec::new();
    files(&root.join("src/svg/tests"), "svg", &mut inputs)?;
    inputs.sort();
    if let Ok(filter) = std::env::var("TILEINK_METAL_SVG_FILTER") {
        inputs.retain(|p| p.to_string_lossy().contains(&filter));
    }
    assert!(!inputs.is_empty());
    let mut options = usvg::Options::default();
    options
        .fontdb_mut()
        .load_fonts_dir(root.join("src/svg/fonts"));
    #[cfg(feature = "wgpu")]
    let (device, queue) = {
        let (identity, device, queue) =
            gpu::device("metal", false, false, wgpu::MemoryHints::MemoryUsage);
        std::fs::write(
            directory.join("identity.json"),
            serde_json::to_vec_pretty(&identity)?,
        )?;
        (device, queue)
    };
    #[cfg(feature = "wgpu")]
    let mut renderer = tileink::WgpuRenderer::new(&device, &queue, 1, 1, Color::TRANSPARENT);
    #[cfg(feature = "metal")]
    let context = {
        let identity: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("identity.json"))?)?;
        tileink::NativeContext::new(
            tileink::NativeBackend::Metal,
            &tileink::NativeContextOptions {
                physical_adapter: Some(
                    identity["physical_identity"]
                        .as_str()
                        .ok_or("missing physical identity")?
                        .into(),
                ),
                validation: true,
            },
        )?
    };
    #[cfg(feature = "metal")]
    let mut renderer = tileink::NativeRenderer::with_context(&context, 1, 1)?;
    #[cfg(feature = "metal")]
    renderer.set_clear_color(Color::TRANSPARENT);
    #[allow(unused_mut)]
    let mut failures: Vec<String> = Vec::new();
    let mut results = Vec::new();
    for (index, path) in inputs.iter().enumerate() {
        let relative = path.strip_prefix(root.join("src/svg/tests"))?;
        let output = directory.join(relative).with_extension("bin");
        std::fs::create_dir_all(output.parent().unwrap())?;
        let input = std::fs::read(path)?;
        options.resources_dir = path.parent().map(Path::to_path_buf);
        let tree = usvg::Tree::from_data(&input, &options)?;
        let size = tree
            .size()
            .to_int_size()
            .scale_to_width(300)
            .ok_or("invalid SVG size")?;
        let mut canvas = Canvas::new(size.width(), size.height(), 1.0);
        canvas.push_svg_with_options(
            &tree,
            SvgOptions {
                transform: Affine::scale_non_uniform(
                    size.width() as f64 / tree.size().width() as f64,
                    size.height() as f64 / tree.size().height() as f64,
                ),
                ..Default::default()
            },
        )?;
        #[cfg(feature = "wgpu")]
        let image = {
            renderer.render(&canvas);
            renderer.image()
        };
        #[cfg(feature = "metal")]
        let image = match renderer
            .render_to_image(&canvas)
            .and_then(|pending| pending.readback())
        {
            Ok(image) => image,
            Err(error) => {
                failures.push(format!("{}: {error}", relative.display()));
                continue;
            }
        };
        let pixels: &[u8] = bytemuck::cast_slice(&image.pixels);
        #[cfg(feature = "wgpu")]
        {
            std::fs::write(&output, pixels)?;
            std::fs::write(output.with_extension("sha256"), digest(&input))?;
        }
        #[cfg(feature = "metal")]
        {
            assert_eq!(
                std::fs::read_to_string(output.with_extension("sha256"))?,
                digest(&input),
                "SVG input changed"
            );
            let expected = std::fs::read(&output)?;
            let count = pixels.iter().zip(&expected).filter(|(a, b)| a != b).count();
            if count != 0 || pixels.len() != expected.len() {
                std::fs::write(output.with_extension("native.bin"), pixels)?;
                failures.push(format!("{}: {count} differing bytes", relative.display()));
            }
        }
        results.push(serde_json::json!({"input":relative,"width":image.width,"height":image.height,"sha256":digest(pixels)}));
        if index % 100 == 0 {
            eprintln!("SVG {}/{}: {}", index + 1, inputs.len(), relative.display());
        }
    }
    #[cfg(feature = "metal")]
    context.check_validation()?;
    let route = if cfg!(feature = "metal") {
        "native"
    } else {
        "wgpu"
    };
    std::fs::write(
        directory.join(format!("{route}-results.json")),
        serde_json::to_vec_pretty(&serde_json::json!({"results":results,"failures":failures}))?,
    )?;
    assert!(
        failures.is_empty(),
        "{} failed SVGs:\n{}",
        failures.len(),
        failures.join("\n")
    );
    Ok(())
}
