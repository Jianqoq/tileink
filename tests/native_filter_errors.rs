#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#![cfg(all(feature = "metal", target_os = "macos"))]
use tileink::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn invalid_filter_graph_preserves_the_actual_error() -> Result<(), Box<dyn std::error::Error>> {
    let context = NativeContext::new(
        NativeBackend::Metal,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?;
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_backdrop_layer(
        Filter::Graph {
            fixed_region: true,
            primitives: vec![FilterPrimitive {
                input: FilterInput::Primitive(0),
                input2: None,
                region: Bounds::canvas(32, 32),
                linear_rgb: false,
                kind: FilterPrimitiveKind::Identity,
            }],
        },
        Region::rect(peniko::kurbo::Rect::new(0., 0., 32., 32.), Radius::ZERO),
    );
    canvas.pop_layer();
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    let error = renderer.render(&canvas).unwrap_err().to_string();
    assert!(error.contains("missing or forward primitive"), "{error}");
    context.check_validation()?;
    Ok(())
}
