use super::{Result, common::capture, evidence, example_suite, gpu, report::Report};
use std::{path::Path, rc::Rc};

pub fn render(
    inputs: Rc<capture::Inputs>,
    routes: &mut [gpu::Route],
    native_routes: &super::native::Routes,
    report: &mut Report,
    output: &Path,
) -> Result<()> {
    let mut captured = Vec::new();
    let mut pipelines = Vec::new();
    for route in routes {
        println!("Rendering all examples through {}", route.name);
        let frames = capture::run(
            route.renderer.device(),
            route.renderer.queue(),
            inputs.clone(),
            example_suite::OUTPUTS,
            example_suite::run,
        )?;
        route.verify_fine_compiler(frames.precompiled_dxil_seen)?;
        pipelines.push(serde_json::json!({"route":route.name,"workloads":frames.pipelines}));
        captured.push(frames.images);
    }
    for (name, frames) in native_routes.examples(inputs)? {
        pipelines.push(serde_json::json!({"route": name, "workloads": frames.pipelines}));
        captured.push(frames.images);
    }
    evidence::write_new_json(
        &output.join("example-pipelines.json"),
        &serde_json::json!(pipelines),
    )?;
    for name in example_suite::OUTPUTS {
        let images = captured
            .iter_mut()
            .map(|frames| {
                frames
                    .remove(*name)
                    .ok_or_else(|| format!("missing captured example {name}").into())
            })
            .collect::<Result<Vec<_>>>()?;
        report.record(name, &images)?;
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires hardware DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
    fn example_capture_is_scoped_and_uses_the_supplied_device() -> Result<()> {
        let instance = super::super::probe::instance()?;
        let route = gpu::create(&instance, wgpu::Backend::Dx12, false, None)?;
        let inputs = Rc::new(capture::Inputs {
            fonts: super::super::common::fonts::Snapshot::system()?,
            svgs: Default::default(),
        });
        let device = route.renderer.device();
        let queue = route.renderer.queue();
        let names = ["empty"];
        let captured = capture::run(device, queue, inputs.clone(), &names, || {
            assert!(capture::run(device, queue, inputs.clone(), &names, || Ok(())).is_err());
            super::super::common::render_to_png_wgpu_with(
                "empty",
                17,
                15,
                peniko::Color::TRANSPARENT,
                |renderer| {
                    assert_eq!(
                        super::super::common::new_wgpu_renderer(1, 1, peniko::Color::TRANSPARENT)
                            .device(),
                        device
                    );
                    renderer.render(&tileink::Canvas::new(17, 15, 1.0))?;
                    Ok(())
                },
            )
        })?;
        assert_eq!(captured.images["empty"].pixels, vec![0; 17 * 15]);
        assert_eq!(captured.pipelines.len(), 1);
        let error = capture::run(device, queue, inputs.clone(), &names, || {
            Err("injected scene failure".into())
        });
        assert!(error.is_err());
        let recovered = capture::run(device, queue, inputs, &names, || {
            super::super::common::render_to_png_wgpu_with(
                "empty",
                1,
                1,
                peniko::Color::TRANSPARENT,
                |renderer| {
                    renderer.render(&tileink::Canvas::new(1, 1, 1.0))?;
                    Ok(())
                },
            )
        })?;
        assert_eq!(recovered.images["empty"].pixels, [0]);
        Ok(())
    }
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    #[test]
    fn example_svg_uses_frozen_tree_and_checks_original_file() -> Result<()> {
        let input =
            std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(
                format!(
                    "captured-example-{}-{}.svg",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                ),
            ))?;
        std::fs::write(
            &input,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="5"><path d="M0 0H10V5H0Z" fill="red"/></svg>"#,
        )?;
        let corpus = super::super::svg::Corpus::load(std::slice::from_ref(&input))?;
        let fonts = super::super::common::fonts::Snapshot::system()?;
        let captured = capture::Inputs {
            fonts,
            svgs: std::iter::once((input.clone(), corpus.trees[0].clone())).collect(),
        };
        std::fs::write(
            &input,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="5" height="10"><path d="M0 0H5V10H0Z" fill="blue"/></svg>"#,
        )?;
        let tree = captured.svg(&input)?;
        assert_eq!(tree.size().width(), 10.0);
        assert_eq!(tree.size().height(), 5.0);
        assert!(corpus.verify_unchanged().is_err());
        assert!(
            captured
                .svg(&input.with_extension("uncaptured.svg"))
                .is_err()
        );
        std::fs::remove_file(&input)?;
        Ok(())
    }
}
