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
fn zero_length_dashes_respect_line_caps() -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    for (cap, expected) in [
        ("round", [0, 128, 0, 255]),
        ("square", [0, 128, 0, 255]),
        ("butt", [0, 0, 0, 0]),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/svg/tests/painting/stroke-dasharray")
            .join(format!("0-n-with-{cap}-caps.svg"));
        let (scene, width, height) = common::load_svg_scene(path, 300)?;
        let mut renderer = NativeRenderer::with_context(&context, width, height)?;
        let image = renderer.render_to_image(&scene)?.readback()?;
        for (x, y) in [(60, 60), (120, 60), (60, 120), (240, 240)] {
            let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
            assert_eq!(actual, expected, "{cap} dash at ({x}, {y})");
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn miter_clip_stroke_keeps_dash_gaps() -> Result<(), Box<dyn std::error::Error>> {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100">
        <path d="M 20 50 H 180" fill="none" stroke="green" stroke-width="10"
              stroke-linejoin="miter-clip" stroke-dasharray="20 20"/>
    </svg>"#;
    let tree = usvg::Tree::from_data(svg, &common::svg_options())?;
    let (scene, width, height) = common::svg_tree_to_scene(&tree, 200)?;
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    let mut renderer = NativeRenderer::with_context(&context, width, height)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    assert_eq!(
        image.pixels[(50 * width + 30) as usize].to_le_bytes(),
        [0, 128, 0, 255]
    );
    assert_eq!(
        image.pixels[(50 * width + 50) as usize].to_le_bytes(),
        [0, 0, 0, 0]
    );
    Ok(())
}

#[test]
#[ignore = "requires a native GPU; run explicitly with --ignored"]
fn zero_length_subpaths_respect_line_caps() -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    for (cap, expected) in [
        ("round", [0, 128, 0, 255]),
        ("square", [0, 128, 0, 255]),
        ("butt", [0, 0, 0, 0]),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/svg/tests/painting/stroke-linecap")
            .join(format!("zero-length-path-with-{cap}.svg"));
        let (scene, width, height) = common::load_svg_scene(path, 300)?;
        let mut renderer = NativeRenderer::with_context(&context, width, height)?;
        let image = renderer.render_to_image(&scene)?.readback()?;
        for (x, y) in [(150, 105), (105, 150), (195, 150), (150, 195)] {
            let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
            assert_eq!(actual, expected, "{cap} zero-length path at ({x}, {y})");
        }
    }
    Ok(())
}
