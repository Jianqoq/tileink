use super::{Result, common, gpu, pixels, svg};

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn repeated_filter_frames_are_exact_across_apis_and_texture_modes() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/tests/filters");
    let corpus = svg::Corpus::load(&[
        root.join("feMorphology/huge-radius.svg"),
        root.join("feComposite/operator=out.svg"),
    ])?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
        backend_options: wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: wgpu::Dx12Compiler::DynamicDxc {
                    dxc_path: std::env::var("TILEINK_PARITY_DXCOMPILER")?,
                },
                ..Default::default()
            },
            ..Default::default()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut routes = Vec::new();
    let mut luid = None;
    for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
        for portable in [false, true] {
            let route = gpu::create(&instance, backend, portable, luid.as_deref())?;
            luid = Some(route.identity.clone());
            routes.push(route);
        }
    }
    let mut expected = [None, None];
    // Write-only UAV hazards appeared only after repeated frames on the same
    // renderer. Keep both identical repeats and scene changes: a fresh renderer
    // per fixture hid the missing DX12 barrier. All four routes and all RGBA bytes
    // must match, including each scene's first frame after different prior content.
    for frame in 0..32 {
        let case = if frame < 16 { 0 } else { frame % 2 };
        let (canvas, _, _) = common::svg_tree_to_scene(&corpus.trees[case], 300)?;
        for route in &mut routes {
            route.renderer.render(&canvas);
            let actual = route.renderer.image();
            if let Some(reference) = &expected[case] {
                let difference = pixels::compare(reference, &actual)?;
                assert_eq!(
                    difference.pixels, 0,
                    "frame {frame}, case {case}, {}: {difference:?}",
                    route.name
                );
            } else {
                expected[case] = Some(actual);
            }
        }
    }
    corpus.verify_unchanged()?;
    Ok(())
}
