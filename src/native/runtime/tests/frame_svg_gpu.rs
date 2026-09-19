use super::fine_fixture::routes;
use crate::Radius;
use crate::shared::layer::{
    filter::{Filter, MorphologyOperator},
    region::Region,
};
use crate::{
    Canvas, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer, SvgOptions,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn four_api_frame_svg_filter_boundaries_preserve_pixels() -> Result<()> {
    let routes = routes()?;
    let mut renderers = Vec::new();
    for backend in [NativeBackend::Dx12, NativeBackend::Vulkan] {
        let context = NativeContext::new(
            backend,
            &NativeContextOptions {
                physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
                validation: true,
            },
        )?;
        renderers.push(NativeRenderer::with_context(&context, 1, 1)?);
    }
    let mut scenes = Vec::new();
    for operator in [MorphologyOperator::Erode, MorphologyOperator::Dilate] {
        let mut canvas = Canvas::new(11, 9, 1.0);
        canvas.push_filter_layer(
            Filter::Morphology {
                radius_x: 1.0,
                radius_y: 2.0,
                operator,
            },
            Region::rect(Rect::new(0.0, 0.0, 11.0, 9.0), Radius::ZERO),
        );
        canvas.push_rect(
            Rect::new(2.0, 2.0, 9.0, 7.0),
            Radius::ZERO,
            Color::from_rgba8(91, 203, 71, 191),
        );
        canvas.pop_layer();
        scenes.push((format!("morphology {operator:?}"), canvas));
    }
    for (name, svg) in [
        (
            "zero morphology radius",
            include_str!("../../../svg/tests/filters/feMorphology/zero-radius.svg"),
        ),
        (
            "empty tile source",
            include_str!("../../../svg/tests/filters/feTile/empty-region.svg"),
        ),
        (
            "negative convolution bias",
            include_str!("../../../svg/tests/filters/feConvolveMatrix/bias=-0.5.svg"),
        ),
    ] {
        let tree = usvg::Tree::from_str(svg, &usvg::Options::default())?;
        let mut canvas = Canvas::new(300, 300, 1.0);
        canvas.push_svg_with_options(
            &tree,
            SvgOptions {
                transform: Affine::scale(1.5),
                ..Default::default()
            },
        )?;
        scenes.push((name.to_owned(), canvas));
    }
    for (name, canvas) in scenes {
        let expected = routes.canvas_reference(&canvas)?;
        for renderer in &mut renderers {
            let image = renderer.render_to_image(&canvas)?.readback()?;
            let actual = bytemuck::cast_slice::<_, u8>(&image.pixels);
            assert_eq!(actual.len(), expected[0].len(), "{name} byte count");
            if let Some(pixel) = actual
                .chunks_exact(4)
                .zip(expected[0].chunks_exact(4))
                .position(|(a, b)| a != b)
            {
                panic!(
                    "{name} {:?} pixel {pixel}: {:?} != {:?}",
                    renderer.context().backend(),
                    &actual[pixel * 4..pixel * 4 + 4],
                    &expected[0][pixel * 4..pixel * 4 + 4]
                );
            }
            renderer.context().check_validation()?;
        }
    }
    routes.validate()
}
