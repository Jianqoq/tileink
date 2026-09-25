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
fn svg_text_uses_fallback_and_bold_fonts() -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(backend(), &NativeContextOptions::default())?;
    for (fixture, sample) in [
        ("font/simple-case.svg", (60, 143)),
        ("font-family/bold-sans-serif.svg", (100, 130)),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/svg/tests/text")
            .join(fixture);
        let (scene, width, height) = common::load_svg_scene(path, 300)?;
        let mut renderer = NativeRenderer::with_context(&context, width, height)?;
        let image = renderer.render_to_image(&scene)?.readback()?;
        let (x, y) = sample;
        let actual = image.pixels[(y * width + x) as usize].to_le_bytes();
        assert!(
            actual[..3] == [0, 0, 0] && actual[3] >= 200,
            "{fixture} at ({x}, {y}): expected visible black text, got {actual:?}"
        );
    }
    Ok(())
}
