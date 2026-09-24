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

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn svg_filter_color_interpolation_matches_reference_pixels()
-> Result<(), Box<dyn std::error::Error>> {
    // Interior pixels avoid edge coverage differences and exercise the filter math itself.
    let cases = [
        // Native readback is premultiplied; the PNG's straight [229, 255, 196, 204]
        // corresponds to this stored pixel.
        (
            "feComponentTransfer/mixed-types.svg",
            100,
            100,
            [183, 204, 157, 204],
        ),
        (
            "feComponentTransfer/type=table-on-blue.svg",
            100,
            100,
            [170, 187, 120, 255],
        ),
        (
            "feComponentTransfer/type=table-on-blue-with-sRGB-interpolation.svg",
            100,
            100,
            [170, 187, 0, 255],
        ),
        (
            "feBlend/with-subregion-on-input-1.svg",
            100,
            100,
            [28, 101, 86, 255],
        ),
        (
            "feBlend/with-subregion-on-input-2.svg",
            100,
            100,
            [28, 101, 86, 255],
        ),
        ("feBlend/mode=screen.svg", 100, 100, [46, 173, 86, 255]),
        (
            "feComposite/with-subregion-on-input-1.svg",
            75,
            75,
            [28, 101, 196, 255],
        ),
        (
            "feComposite/operator=arithmetic-with-large-k1-4.svg",
            30,
            30,
            [81, 228, 255, 255],
        ),
        (
            "feComposite/operator=arithmetic-with-opacity.svg",
            30,
            30,
            [165, 177, 195, 239],
        ),
        (
            "feComposite/operator=arithmetic.svg",
            30,
            30,
            [171, 183, 209, 255],
        ),
        (
            "feComposite/operator=arithmetic-on-sRGB.svg",
            30,
            30,
            [115, 143, 187, 255],
        ),
        (
            "feComposite/operator=arithmetic-with-opacity-on-sRGB.svg",
            30,
            30,
            [115, 143, 175, 239],
        ),
    ];
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/tests/filters");
    for (name, x, y, expected) in cases {
        let (scene, width, height) = common::load_svg_scene(root.join(name), 300)?;
        let mut renderer = NativeRenderer::with_context(&context, width, height)?;
        let image = renderer.render_to_image(&scene)?.readback()?;
        let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
        assert!(
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
            "{name}: expected {expected:?}, got {actual:?}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn svg_convolve_bias_matches_reference_pixels() -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests/filters/feConvolveMatrix/bias=0.5.svg");
    let (scene, width, height) = common::load_svg_scene(path, 300)?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    for (x, y, expected) in [(10, 10, [94, 94, 94, 128]), (39, 39, [255, 255, 207, 255])] {
        let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
        assert!(
            actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
            "({x},{y}): expected {expected:?}, got {actual:?}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn svg_point_light_tracks_viewbox_and_shape_transforms() -> Result<(), Box<dyn std::error::Error>> {
    // SVG light positions are in the filtered element's user space; lighting runs in canvas pixels.
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/svg/tests/filters/fePointLight");
    for (name, samples) in [
        ("custom-attributes.svg", [(150, 210, 255), (100, 140, 41)]),
        ("complex-transform.svg", [(173, 205, 255), (100, 100, 18)]),
    ] {
        let (scene, width, height) = common::load_svg_scene(root.join(name), 300)?;
        let mut renderer = NativeRenderer::with_context(&context, width, height)?;
        let image = renderer.render_to_image(&scene)?.readback()?;
        for (x, y, expected) in samples {
            let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
            assert!(
                actual[0].abs_diff(expected) <= 2,
                "{name} ({x},{y}): expected {expected}, got {actual:?}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn svg_negative_spot_cone_angle_still_limits_light() -> Result<(), Box<dyn std::error::Error>> {
    // A signed cone angle has the same cosine cutoff; it must not mean "no cone".
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests/filters/feSpotLight/limitingConeAngle=-30.svg");
    let (scene, width, height) = common::load_svg_scene(path, 300)?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    for (x, y, expected) in [(200, 150, 0), (80, 150, 115)] {
        let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
        assert!(
            actual[0].abs_diff(expected) <= 2,
            "({x},{y}): expected {expected}, got {actual:?}"
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn svg_pattern_tracks_viewbox_scale() -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests/paint-servers/pattern/simple-case.svg");
    let (scene, width, height) = common::load_svg_scene(path, 300)?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    for (x, y, expected) in [(45, 45, [0, 128, 0, 255]), (65, 45, [0, 0, 0, 0])] {
        let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
        assert_eq!(actual, expected, "({x},{y})");
    }
    let offset_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests/paint-servers/pattern/with-x-and-y.svg");
    let (scene, width, height) = common::load_svg_scene(offset_path, 300)?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    assert_eq!(
        image.pixels[(45 * width + 45) as usize].to_le_bytes(),
        [128, 128, 128, 255]
    );
    let viewbox_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests/paint-servers/pattern/patternContentUnits-with-viewBox.svg");
    let (scene, width, height) = common::load_svg_scene(viewbox_path, 300)?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    assert_eq!(
        image.pixels[(35 * width + 67) as usize].to_le_bytes(),
        [128, 128, 128, 255]
    );
    let parsed_viewbox_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/svg/tests/paint-servers/pattern/with-viewBox.svg");
    let (scene, width, height) = common::load_svg_scene(parsed_viewbox_path, 300)?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    assert_eq!(
        image.pixels[(75 * width + 31) as usize].to_le_bytes(),
        [128, 128, 128, 255]
    );
    Ok(())
}
