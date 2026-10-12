#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "Test setup and assertions abort on failure; production builds are checked separately"
    )
)]
#![cfg(any(feature = "dx12", feature = "vulkan", feature = "metal"))]
use peniko::{Color, kurbo::Point};
use tileink::*;

#[test]
#[ignore = "requires explicitly pinned physical GPU"]
fn appended_text_preserves_fractional_pixels_after_large_coordinate_cancellation()
-> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "metal")]
    let backend = NativeBackend::Metal;
    #[cfg(feature = "vulkan")]
    let backend = NativeBackend::Vulkan;
    #[cfg(feature = "dx12")]
    let backend = NativeBackend::Dx12;
    #[cfg(feature = "dx12")]
    // SAFETY: The serial GPU fixture enables validation before creating its context; the creation gate prevents late debug-layer changes.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let context = NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_TEST_GPU")?),
            validation: true,
        },
    )?;
    let mut renderer = NativeRenderer::with_context(&context, 128, 96)?;
    let mut fonts = TextFontSystem::new_with_locale_and_db(
        "en-US".into(),
        cosmic_text::fontdb::Database::default(),
    );
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../src/svg/fonts/NotoSans-Regular.ttf").to_vec());
    fonts.db_mut().set_sans_serif_family("Noto Sans");
    let mut text = TextContext::new();
    let layout = text.layout(&mut fonts, TextLayoutOptions::new("Hi", 20.0));
    assert!(!layout.is_empty());
    let mut leaf = Canvas::new(128, 96, 1.0);
    leaf.push_text_layout(&layout, Point::new(30.0, 30.0), Color::WHITE)
        .unwrap();
    for offset in [-0.5, -0.25, 0.25, 0.5] {
        let mut expected = Canvas::new(128, 96, 1.0);
        expected.append(&leaf, Point::new(offset, offset));
        let expected = renderer
            .render_to_image_with_text(&expected, &mut fonts, &mut text)?
            .readback()?;
        assert!(expected.pixels.iter().any(|pixel| *pixel != 0));
        let large = 16_777_216.0;
        let mut intermediate = Canvas::new(128, 96, 1.0);
        intermediate.append(&leaf, Point::new(large + offset, large + offset));
        let mut actual = Canvas::new(128, 96, 1.0);
        actual.append(&intermediate, Point::new(-large, -large));
        let actual = renderer
            .render_to_image_with_text(&actual, &mut fonts, &mut text)?
            .readback()?;
        assert!(actual.pixels == expected.pixels, "offset={offset}");
    }
    for origin in [-f64::MAX, -3.0e9, 3.0e9, f64::MAX] {
        let mut canvas = Canvas::new(128, 96, 1.0);
        assert!(
            canvas
                .push_text_layout(&layout, Point::new(origin, origin), Color::WHITE)
                .is_none()
        );
        let image = renderer
            .render_to_image_with_text(&canvas, &mut fonts, &mut text)?
            .readback()?;
        assert!(image.pixels.iter().all(|pixel| *pixel == 0));
    }
    context.check_validation()?;
    Ok(())
}
