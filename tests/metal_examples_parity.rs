//! Executes the actual example catalog on each exclusive renderer build.
#![cfg(all(target_os = "macos", any(feature = "wgpu", feature = "metal")))]
#[path = "../examples/common/mod.rs"]
mod common;
#[path = "../examples/wgpu/suite.rs"]
mod example_suite;
#[cfg(feature = "wgpu")]
#[allow(dead_code)]
#[path = "../examples/common/benchmark_gpu.rs"]
mod gpu;
#[path = "../examples/common/layer_filter_scenes.rs"]
mod layer_filter_scenes;

#[test]
#[ignore = "all examples on the same physical GPU; run native Metal certification script"]
fn complete_example_catalog_matches_same_device() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let directory = root.join("target/metal-validation/examples");
    std::fs::create_dir_all(&directory)?;
    let fonts = common::fonts::Snapshot::system()?;
    let mut svgs = std::collections::BTreeMap::new();
    let mut options = common::svg_options();
    for input in example_suite::SVG_INPUTS {
        let path = common::example_asset(input);
        options.resources_dir = path.parent().map(std::path::Path::to_path_buf);
        svgs.insert(
            path.clone(),
            usvg::Tree::from_data(&std::fs::read(&path)?, &options)?,
        );
    }
    #[cfg(feature = "wgpu")]
    std::fs::write(
        directory.join("fonts.json"),
        serde_json::to_vec_pretty(&fonts.manifest)?,
    )?;
    #[cfg(feature = "metal")]
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(directory.join("fonts.json"))?)?,
        fonts.manifest,
        "font input changed"
    );
    let inputs = std::rc::Rc::new(common::capture::Inputs { fonts, svgs });
    #[cfg(feature = "wgpu")]
    let captured = {
        let (identity, device, queue) =
            gpu::device("metal", false, false, wgpu::MemoryHints::MemoryUsage);
        std::fs::write(
            directory.join("identity.json"),
            serde_json::to_vec_pretty(&identity)?,
        )?;
        common::capture::run(
            &device,
            &queue,
            inputs,
            example_suite::OUTPUTS,
            example_suite::run,
        )?
    };
    #[cfg(feature = "metal")]
    let captured = {
        let identity: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("identity.json"))?)?;
        let context = tileink::NativeContext::new(
            tileink::NativeBackend::Metal,
            &tileink::NativeContextOptions {
                physical_adapter: Some(
                    identity["physical_identity"]
                        .as_str()
                        .ok_or("missing identity")?
                        .into(),
                ),
                validation: true,
            },
        )?;
        let captured = common::capture::run_native(
            &context,
            inputs,
            example_suite::OUTPUTS,
            example_suite::run,
        )?;
        context.check_validation()?;
        captured
    };
    #[allow(unused_mut)]
    let mut failures: Vec<String> = Vec::new();
    for (name, image) in captured.images {
        let path = directory.join(format!("{name}.bin"));
        let pixels: &[u8] = bytemuck::cast_slice(&image.pixels);
        #[cfg(feature = "wgpu")]
        std::fs::write(path, pixels)?;
        #[cfg(feature = "metal")]
        {
            let expected = std::fs::read(&path)?;
            let count = pixels.iter().zip(&expected).filter(|(a, b)| a != b).count();
            if count > 0 || pixels.len() != expected.len() {
                std::fs::write(path.with_extension("native.bin"), pixels)?;
                failures.push(format!("{name}: {count} differing bytes"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}
