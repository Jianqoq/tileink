//! Regression for colored backdrop pixels at a Gaussian accumulation boundary.
use super::common::liquid_glass_fast_path as scenes;
use super::{Result, gpu, pixels, probe};

#[test]
#[ignore = "requires same-GPU DX12/Vulkan and TILEINK_PARITY_DXCOMPILER"]
fn mixed_backdrop_blur_matches_all_routes() -> Result<()> {
    let instance = probe::instance()?;
    let scenes = [
        ("mixed", scenes::mixed_scene()),
        (
            "isolated blur",
            scenes::single_mode_scene(scenes::GlassMode::Blur),
        ),
    ];
    let mut reference: Option<Vec<tileink::Image>> = None;
    let mut identity = None;
    for portable in [false, true] {
        for backend in [wgpu::Backend::Dx12, wgpu::Backend::Vulkan] {
            let mut route = gpu::create(&instance, backend, portable, identity.as_deref())?;
            identity = Some(route.identity.clone());
            route.renderer.set_clear_color(peniko::Color::WHITE);
            let mut images = Vec::new();
            for (name, scene) in &scenes {
                route.renderer.render(scene);
                let image = route.renderer.image();
                if let Some(expected) = &reference {
                    let diff = pixels::compare(&expected[images.len()], &image)?;
                    assert_eq!(diff.pixels, 0, "{} / {name}: {diff:?}", route.name);
                }
                images.push(image);
            }
            if reference.is_none() {
                reference = Some(images);
            }
        }
    }
    Ok(())
}
