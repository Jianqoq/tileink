use super::{Result, evidence, gpu, probe, report};
use peniko::{Color, kurbo::Rect};
use tileink::{BlurSampling, Canvas, Filter, Radius, Region};

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn blur_radius_boundaries_match_all_routes() -> Result<()> {
    // Cover the shared-tile boundary and odd/even paired kernels through large
    // radii, with partial workgroups and nonzero source origins.
    let deviations = [0.0_f32, 0.001, 5.0, 5.25, 5.5, 85.0, 85.25, 85.5];
    let scenes: Vec<_> = deviations
        .iter()
        .map(|std_dev| {
            let mut scene = Canvas::new(129, 97, 1.0);
            scene.push_filter_layer(
                Filter::Blur {
                    std_dev_x: *std_dev,
                    std_dev_y: *std_dev,
                    sampling: BlurSampling::FULL_RES,
                },
                Region::rect(Rect::new(8.0, 9.0, 112.0, 81.0), Radius::ZERO),
            );
            for index in 0..6 {
                let x = 11.25 + f64::from(index) * 14.0;
                scene.push_rect(
                    Rect::new(x, 17.5 + f64::from(index), x + 9.75, 72.25),
                    Radius::all(2.25),
                    Color::from_rgba8(30 + index * 31, 217 - index * 23, 97, 61 + index * 29),
                );
            }
            scene.pop_layer();
            scene
        })
        .collect();
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
            "blur-radius-prerequisite-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos(),
            std::process::id()
        ));
    std::fs::create_dir_all(output.parent().unwrap())?;
    std::fs::create_dir(&output)?;
    let cases: Vec<_> = deviations
        .iter()
        .map(|deviation| format!("sigma-{deviation}"))
        .collect();
    evidence::write_new_json(
        &output.join("manifest.json"),
        &serde_json::json!({
            "cases": cases, "expected_frames": cases.len(),
            "sources_and_resources": evidence::source_snapshot(&[], &output)?,
            "binary_sha256": evidence::digest_file(&std::env::current_exe()?)?,
            "format": "all raw premultiplied RGBA channels",
        }),
    )?;
    let mut report = report::Report::new(
        &output,
        &cases,
        routes.iter().map(|route| route.metadata.clone()).collect(),
    )?;
    println!("Blur radius report: {}", output.display());
    for (name, scene) in cases.iter().zip(&scenes) {
        let mut images = Vec::new();
        for route in &mut routes {
            route.renderer.render(scene);
            images.push(route.renderer.image());
        }
        report.record(name, &images)?;
        if images
            .iter()
            .any(|image| image.pixels.iter().all(|pixel| *pixel == 0))
        {
            return report
                .finish_checked(Err(format!("{name}: nonempty blur became empty").into()));
        }
    }
    report.finish()
}
