use super::Result;
use crate::{NativeContext, NativeContextOptions, NativeRenderer};

fn assert_svg_pixel(
    relative_path: &str,
    width: u32,
    x: u32,
    y: u32,
    expected: [u8; 4],
) -> Result<()> {
    #[cfg(feature = "dx12")]
    // SAFETY: single-threaded GPU tests enable validation before creating devices.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let context = NativeContext::new(
        super::backend(),
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let data = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path))?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default())?;
    let mut canvas = crate::Canvas::new(width, width, 1.0);
    canvas.push_svg_with_options(
        &tree,
        crate::SvgOptions {
            transform: peniko::kurbo::Affine::scale_non_uniform(
                f64::from(width) / f64::from(tree.size().width()),
                f64::from(width) / f64::from(tree.size().height()),
            ),
            ..Default::default()
        },
    )?;
    let mut renderer = NativeRenderer::with_context(&context, width, width)?;
    let image = renderer.render_to_image(&canvas)?.readback()?;
    let bytes: &[u8] = bytemuck::cast_slice(&image.pixels);
    let offset = ((y * width + x) * 4) as usize;
    assert_eq!(
        &bytes[offset..offset + 4],
        &expected,
        "{relative_path} ({x}, {y})"
    );
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_row_intersection_preserves_half_channel_rounding() -> Result<()> {
    // Propagated precise changed production intersection rounding at coverage 247
    // to 248; the isolated coverage kernel did not reproduce that compilation.
    assert_svg_pixel("examples/tiger.svg", 900, 190, 481, [222, 148, 148, 255])
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_gradient_ramp_preserves_fused_fraction() -> Result<()> {
    // A separately rounded t*63 before subtracting the ramp index produced 238
    // instead of the fused result 237. Explicit fused range reduction makes the contract unambiguous.
    assert_svg_pixel(
        "src/svg/tests/paint-servers/linearGradient/attributes-via-xlink-href.svg",
        300,
        121,
        30,
        [237, 237, 237, 255],
    )
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_tile_edge_area_preserves_half_channel_rounding() -> Result<()> {
    // Identical emitted segments still diverged when integrating a clipped edge.
    assert_svg_pixel(
        "src/svg/tests/shapes/line/simple-case.svg",
        300,
        103,
        132,
        [63, 71, 0, 205],
    )
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_pattern_transform_preserves_product_residual() -> Result<()> {
    assert_svg_pixel(
        "src/svg/tests/structure/image/with-transform.svg",
        300,
        128,
        135,
        [28, 28, 28, 255],
    )
}
