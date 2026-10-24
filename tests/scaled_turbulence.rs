#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#[path = "support/native_gpu.rs"]
mod backend;
#[path = "../examples/common/svg.rs"]
mod common;
#[path = "support/turbulence_reference.rs"]
mod reference;

#[test]
#[ignore = "requires pinned TILEINK_TEST_GPU; run each backend separately"]
fn scaled_turbulence_matches_svg_noise_semantics() {
    let mut gpu = backend::Gpu::new();
    let reference = reference::Noise::new(0);
    for width in [300, 1600] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/svg/tests/filters/feTurbulence/numOctaves=5.svg");
        let (canvas, width, height) = common::load_svg_scene(path, width).unwrap();
        let target = gpu.target(width, height);
        gpu.render_immediate(&canvas, &target);
        let image = gpu.image_target(&target);
        assert_eq!((image.width, image.height), (width, height));
        gpu.render_immediate(&canvas, &target);
        assert_eq!(
            image.pixels,
            gpu.image_target(&target).pixels,
            "same backend must remain deterministic"
        );
        let scale = f64::from(width) / 200.0;
        let start = width / 5;
        let end = width * 4 / 5;
        let mut differences = 0;
        let mut maximum = 0;
        // The SVG algorithm uses real arithmetic. GPU f32 coordinates and pow
        // can straddle an RGBA8 half-channel boundary; permit one byte against
        // this independent f64 oracle rather than pinning an archived GPU hash.
        for y in start..end {
            for x in start..end {
                let expected = reference.pixel(
                    [f64::from(x) / scale, f64::from(y) / scale],
                    0.05,
                    5,
                    false,
                    true,
                );
                let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
                let error = actual
                    .into_iter()
                    .zip(expected)
                    .map(|(a, b)| a.abs_diff(b))
                    .max()
                    .unwrap();
                maximum = maximum.max(error);
                differences += usize::from(error != 0);
                assert!(
                    error <= 1,
                    "{width}px ({x},{y}): {actual:?} != {expected:?}"
                );
            }
        }
        eprintln!("{width}px oracle: {differences} pixels differ; maximum channel delta={maximum}");
        for y in 0..height {
            for x in 0..width {
                if (start..end).contains(&x) && (start..end).contains(&y) {
                    continue;
                }
                let actual = image.pixels[(y * width + x) as usize];
                assert_eq!(
                    actual & 0x00ff_ffff,
                    0,
                    "noise must stay inside its filter region"
                );
                if (scale * 0.5).fract() == 0.0 {
                    // At this integral scale, the SVG frame's outer and inner
                    // stroke edges lie exactly on pixel boundaries.
                    let outer_start = (scale * 0.5) as u32;
                    let inner_start = (scale * 1.5) as u32;
                    let outer = outer_start..width - outer_start;
                    let inner = inner_start..width - inner_start;
                    let border = outer.contains(&x)
                        && outer.contains(&y)
                        && !(inner.contains(&x) && inner.contains(&y));
                    assert_eq!(actual, if border { 0xff00_0000 } else { 0 });
                }
            }
        }
    }
}
