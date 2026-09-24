#![cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]

#[allow(unused_imports)]
#[path = "../examples/common/mod.rs"]
mod common;

use std::path::Path;
use tileink::{NativeBackend, NativeContext, NativeContextOptions, NativeRenderer};

fn backend() -> NativeBackend {
    #[cfg(feature = "dx12")]
    {
        NativeBackend::Dx12
    }
    #[cfg(all(not(feature = "dx12"), feature = "vulkan"))]
    {
        NativeBackend::Vulkan
    }
    #[cfg(all(not(feature = "dx12"), not(feature = "vulkan"), feature = "metal"))]
    {
        NativeBackend::Metal
    }
}

fn assert_svg_pixel(
    relative_path: &str,
    x: u32,
    y: u32,
    expected: [u8; 4],
) -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests/masking")
        .join(relative_path);
    let (scene, width, height) = common::load_svg_scene(path, 300)?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
    assert!(
        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
        "{relative_path} ({x},{y}): expected {expected:?}, got {actual:?}"
    );
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn transformed_text_uses_its_object_bounding_box_clip() -> Result<(), Box<dyn std::error::Error>> {
    assert_svg_pixel(
        "clipPath/clip-path-with-transform-on-text.svg",
        123,
        64,
        [0, 128, 0, 255],
    )
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn rotated_mask_region_tracks_its_target() -> Result<(), Box<dyn std::error::Error>> {
    assert_svg_pixel(
        "mask/half-width-region-with-rotation.svg",
        149,
        45,
        [0, 128, 0, 255],
    )?;
    assert_svg_pixel(
        "mask/half-width-region-with-rotation.svg",
        150,
        150,
        [0, 0, 0, 0],
    )
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn linear_rgb_luminance_mask_keeps_gradient_coverage() -> Result<(), Box<dyn std::error::Error>> {
    // The reference's straight [0, 130, 0, 53] is [0, 27, 0, 53] premultiplied.
    assert_svg_pixel(
        "mask/color-interpolation=linearRGB.svg",
        100,
        100,
        [0, 27, 0, 53],
    )
}
