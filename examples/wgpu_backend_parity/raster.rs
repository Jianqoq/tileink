use super::{Result, common, evidence, gpu, pixels, probe, readback, report, svg};

#[path = "../common/numeric_raster_cases.rs"]
mod raster_cases;

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn numeric_raster_benchmark_inputs_are_pixel_exact() -> Result<()> {
    // Performance results need a pixel-correct workload at the measured sizes,
    // including wide filters; passing only the 300-pixel SVG corpus is insufficient.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/tests");
    let corpus = svg::Corpus::load(&raster_cases::CASES.map(|(_, path)| root.join(path)))?;
    let instance = probe::instance()?;
    let mut identity = None;
    let mut routes = Vec::new();
    for portable in [false, true] {
        for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
            let route = gpu::create(&instance, backend, portable, identity.as_deref())?;
            identity = Some(route.identity.clone());
            routes.push(route);
        }
    }
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/backend-parity")
        .join(format!(
            "numeric-pixel-prerequisite-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            std::process::id(),
        ));
    std::fs::create_dir_all(output.parent().unwrap())?;
    std::fs::create_dir(&output)?;
    let cases: Vec<_> = raster_cases::CASES
        .iter()
        .flat_map(|(name, _)| raster_cases::WIDTHS.map(|width| format!("{name}-{width}")))
        .collect();
    evidence::write_new_json(
        &output.join("manifest.json"),
        &serde_json::json!({
            "cases": cases, "expected_frames": cases.len(), "runtime_resources": corpus.snapshot,
            "sources_and_resources": evidence::source_snapshot(&corpus.resources, &output)?,
            "binary_sha256": evidence::digest_file(&std::env::current_exe()?)?,
            "format": "raw Rgba8Unorm, all channels, external application target and owned-target equivalence",
        }),
    )?;
    let mut report = report::Report::new(
        &output,
        &cases,
        routes.iter().map(|route| route.metadata.clone()).collect(),
    )?;
    println!("Pixel prerequisite report: {}", output.display());
    let mut target_errors = Vec::new();
    for ((name, _), tree) in raster_cases::CASES.iter().zip(&corpus.trees) {
        for width in raster_cases::WIDTHS {
            let (canvas, width, height) = common::svg_tree_to_scene(tree, width)?;
            let mut images = Vec::new();
            for route in &mut routes {
                route.renderer.render(&canvas);
                let owned = route.renderer.image();
                let target = route
                    .renderer
                    .device()
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("numeric Criterion pixel prerequisite"),
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
                            | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                route.renderer.render_to_wgpu_texture(&canvas, &target)?;
                let actual =
                    readback::rgba8(route.renderer.device(), route.renderer.queue(), &target)?;
                let target_difference = pixels::compare(&owned, &actual)?;
                if target_difference.pixels != 0 {
                    target_errors.push(format!(
                        "{name}-{width}, {} owned/external: {target_difference:?}",
                        route.name
                    ));
                    pixels::save_raw(
                        &owned,
                        &output
                            .join("owned")
                            .join(&route.name)
                            .join(format!("{name}-{width}.png")),
                    )?;
                }
                images.push(actual);
            }
            report.record(&format!("{name}-{width}"), &images)?;
        }
    }
    corpus.verify_unchanged()?;
    if !target_errors.is_empty() {
        let error = target_errors.join("; ");
        report.fail(&error)?;
        return Err(error.into());
    }
    report.finish()
}
