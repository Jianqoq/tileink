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
fn symbol_clip_follows_use_transform() -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    for (fixture, sample, expected) in [
        (
            "indirect-symbol-reference.svg",
            (180, 150),
            [0, 128, 0, 255],
        ),
        ("with-transform-on-use.svg", (180, 120), [0, 128, 0, 255]),
        ("content-outside-the-viewbox.svg", (50, 106), [0, 0, 0, 0]),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/svg/tests/structure/symbol")
            .join(fixture);
        let (scene, width, height) = common::load_svg_scene(path, 300)?;
        let mut renderer = NativeRenderer::with_context(&context, width, height)?;
        let image = renderer.render_to_image(&scene)?.readback()?;
        let (x, y) = sample;
        let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
        assert_eq!(actual, expected, "{fixture} at ({x}, {y})");
    }
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn symbol_default_overflow_clips_to_use_viewport() -> Result<(), Box<dyn std::error::Error>> {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200">
        <symbol id="symbol">
            <rect x="20" y="20" width="160" height="160" fill="green"/>
        </symbol>
        <use href="#symbol" width="100" height="100"/>
    </svg>"##;
    let tree = usvg::Tree::from_data(svg, &common::svg_options())?;
    let (scene, width, height) = common::svg_tree_to_scene(&tree, 200)?;
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    assert_eq!(
        image.pixels[(50 * width + 150) as usize].to_le_bytes(),
        [0, 0, 0, 0]
    );
    Ok(())
}
