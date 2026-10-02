#![cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
use peniko::kurbo::{Affine, Point};
use tileink::*;

#[test]
#[ignore = "requires a native GPU"]
fn transformed_glyphs_preserve_eighth_pixel_motion() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    let context = NativeContext::new(backend, &NativeContextOptions::default())?;
    let mut renderer = NativeRenderer::with_context(&context, 128, 96)?;
    let mut fonts = TextFontSystem::new();
    fonts.db_mut().load_font_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/svg/fonts/NotoSans-Regular.ttf"
    ))?;
    let mut text = TextContext::new();
    // Use linear mask coverage for centroid accuracy; RGB coverage combines channels nonlinearly.
    text.set_raster_options(
        TextRasterOptions::default()
            .with_subpixel_mode(TextSubpixelMode::None)
            .with_composite_mode(TextCompositeMode::Srgb),
    );
    let layout = text.layout(
        &mut fonts,
        TextLayoutOptions::new("Hi", 20.0)
            .with_attrs(TextAttrs::new().family(TextFamily::Name("Noto Sans"))),
    );
    assert!(!layout.is_empty());
    let mut leaf = Canvas::new(128, 96, 1.0);
    leaf.push_text_layout(&layout, Point::new(30.0, 30.0), peniko::Color::WHITE);
    // gfx_ui keeps the shaped leaf stable and animates its retained node transform.
    let root = RetainedNodeId::for_owner(1);
    let child = RetainedNodeId::for_owner(2);
    let mut scene = RetainedScene::new(128, 96, 1.0, root)?;
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            child,
            std::rc::Rc::new(leaf),
            Affine::IDENTITY,
        )
        .commit()?;
    for axis in [0, 1] {
        let mut previous = None;
        for eighth in -4..=8 {
            let offset = f64::from(eighth) * 0.125;
            scene
                .transaction()
                .set_transform(
                    child,
                    Affine::translate(if axis == 0 {
                        (offset, 0.0)
                    } else {
                        (0.0, offset)
                    }),
                )
                .commit()?;
            let image = renderer
                .render_retained_to_image_with_text(&scene, &mut fonts, &mut text)?
                .readback()?;
            let mut mass = 0.0;
            let mut moment = 0.0;
            for (i, pixel) in image.pixels.iter().enumerate() {
                let alpha = f64::from(pixel >> 24);
                mass += alpha;
                moment += alpha
                    * if axis == 0 {
                        (i % 128) as f64
                    } else {
                        (i / 128) as f64
                    };
            }
            assert!(mass > 0.0);
            let center = moment / mass;
            if let Some(old) = previous {
                let delta: f64 = center - old;
                assert!(
                    (delta - 0.125).abs() < 0.04,
                    "axis={axis}, offset={offset}, delta={delta}; fractional motion must not snap"
                );
            }
            previous = Some(center);
        }
    }
    Ok(())
}
