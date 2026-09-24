#![cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]

#[allow(dead_code, unused_imports)]
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

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn svg_filter_color_interpolation_matches_reference_pixels()
-> Result<(), Box<dyn std::error::Error>> {
    // Interior pixels avoid edge coverage differences and exercise the filter math itself.
    let cases = [
        // Native readback is premultiplied; the PNG's straight [229, 255, 196, 204]
        // corresponds to this stored pixel.
        ("feComponentTransfer/mixed-types.svg", [183, 204, 157, 204]),
        (
            "feComponentTransfer/type=table-on-blue.svg",
            [170, 187, 120, 255],
        ),
        (
            "feComponentTransfer/type=table-on-blue-with-sRGB-interpolation.svg",
            [170, 187, 0, 255],
        ),
        ("feBlend/with-subregion-on-input-1.svg", [28, 101, 86, 255]),
        ("feBlend/with-subregion-on-input-2.svg", [28, 101, 86, 255]),
        ("feBlend/mode=screen.svg", [46, 173, 86, 255]),
    ];
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/tests/filters");
    for (name, expected) in cases {
        let (scene, width, height) = common::load_svg_scene(root.join(name), 300)?;
        let mut renderer = NativeRenderer::with_context(&context, width, height)?;
        let image = renderer.render_to_image(&scene)?.readback()?;
        let actual = image.pixels[(100 * width + 100) as usize].to_le_bytes();
        assert!(
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
            "{name}: expected {expected:?}, got {actual:?}"
        );
    }
    Ok(())
}
