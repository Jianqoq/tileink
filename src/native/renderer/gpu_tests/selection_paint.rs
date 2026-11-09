//! Compare one highlighted draw against independently rendered normal/foreground images.
use crate::{
    Canvas, NativeContext, NativeContextOptions, NativeRenderer, Radius, TextContext,
    TextLayoutOptions,
};
use cosmic_text::FontSystem;
use peniko::{
    Color,
    kurbo::{Point, Rect},
};

fn context() -> Result<NativeContext, Box<dyn std::error::Error>> {
    #[cfg(feature = "dx12")]
    // SAFETY: This serial, isolated GPU test enables validation before creating a device.
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    #[cfg(feature = "dx12")]
    let backend = crate::NativeBackend::Dx12;
    #[cfg(feature = "vulkan")]
    let backend = crate::NativeBackend::Vulkan;
    #[cfg(feature = "metal")]
    let backend = crate::NativeBackend::Metal;
    Ok(NativeContext::new(
        backend,
        &NativeContextOptions {
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
            validation: true,
        },
    )?)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU and validation layer; run with --ignored"]
fn highlighted_glyph_pixels_choose_one_brush_without_double_blending()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let mut fonts = FontSystem::new_with_locale_and_db(
        "en-US".into(),
        cosmic_text::fontdb::Database::default(),
    );
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../../../svg/fonts/NotoSans-Regular.ttf").to_vec());
    fonts.db_mut().set_sans_serif_family("Noto Sans");
    let mut text = TextContext::new();
    let layout = text.layout(&mut fonts, TextLayoutOptions::new("MMMM office", 24.));
    let background = Color::from_rgb8(0, 105, 202);
    let normal = Color::from_rgba8(196, 38, 44, 180);
    let ranges = [
        Rect::new(17.25, 8.5, 42.75, 69.25),
        Rect::new(30.5, 12.25, 61.125, 38.75),
        Rect::new(30.5, 12.25, 61.125, 38.75),
    ];
    for scale in [1., 1.25, 2.] {
        for origin in [Point::new(4., 12.), Point::new(4.125, 12.25)] {
            for clip in [None, Some(Rect::new(8.25, 10.5, 100.75, 49.25))] {
                for foreground in [
                    Color::WHITE,
                    Color::TRANSPARENT,
                    Color::from_rgba8(5, 210, 80, 128),
                ] {
                    let mut scenes = Vec::new();
                    for variant in 0..3 {
                        let mut scene = Canvas::new(220, 80, scale);
                        scene.push_rect(Rect::new(0., 0., 220., 80.), Radius::ZERO, background);
                        if variant == 2 {
                            scene.push_text_layout_highlighted(
                                &layout, origin, normal, foreground, &ranges, clip,
                            );
                        } else {
                            let brush = if variant == 0 { normal } else { foreground };
                            if let Some(clip) = clip {
                                scene.push_text_layout_clipped(&layout, origin, clip, brush);
                            } else {
                                scene.push_text_layout(&layout, origin, brush);
                            }
                        }
                        scenes.push(scene);
                    }
                    let mut renderer = NativeRenderer::with_context(
                        &context,
                        scenes[0].physical_width(),
                        scenes[0].physical_height(),
                    )?;
                    let mut images = Vec::new();
                    for scene in &scenes {
                        images.push(
                            renderer
                                .render_to_image_with_text(scene, &mut fonts, &mut text)?
                                .readback()?,
                        );
                    }
                    assert_ne!(
                        images[0].pixels, images[1].pixels,
                        "test requires visible glyph ink"
                    );
                    let width = images[0].width as usize;
                    for (index, actual) in images[2].pixels.iter().enumerate() {
                        let point = Point::new(
                            ((index % width) as f64 + 0.5) / f64::from(scale),
                            ((index / width) as f64 + 0.5) / f64::from(scale),
                        );
                        let selected = ranges.iter().any(|r| {
                            r.x0 <= point.x && point.x < r.x1 && r.y0 <= point.y && point.y < r.y1
                        });
                        let expected = images[usize::from(selected)].pixels[index];
                        assert_eq!(
                            *actual, expected,
                            "scale={scale}, origin={origin:?}, clip={clip:?}, foreground={foreground:?}, point={point:?}"
                        );
                    }
                }
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU and validation layer; run with --ignored"]
fn retained_highlight_brush_domains_follow_inverse_glyph_transform()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{RetainedNodeId, RetainedParent, RetainedScene};
    use peniko::kurbo::Affine;
    use std::rc::Rc;
    let context = context()?;
    let mut fonts = FontSystem::new_with_locale_and_db(
        "en-US".into(),
        cosmic_text::fontdb::Database::default(),
    );
    fonts
        .db_mut()
        .load_font_data(include_bytes!("../../../svg/fonts/NotoSans-Regular.ttf").to_vec());
    fonts.db_mut().set_sans_serif_family("Noto Sans");
    let mut text = TextContext::new();
    let layout = text.layout(&mut fonts, TextLayoutOptions::new("MMMM", 24.));
    let ranges = [Rect::new(17.25, 8.5, 32.75, 50.)];
    for scale in [1., 1.25, 2.] {
        for transform in [
            Affine::translate((3., 4.)),
            Affine::translate((7.25, 3.125)) * Affine::scale(1.15),
            Affine::translate((12., 4.)) * Affine::rotate(0.12),
        ] {
            for foreground in [
                Color::WHITE,
                Color::TRANSPARENT,
                Color::from_rgba8(5, 210, 80, 128),
            ] {
                let mut images = Vec::new();
                for variant in 0..3 {
                    let mut canvas = Canvas::new(140, 80, scale);
                    canvas.push_rect(
                        Rect::new(0., 0., 140., 80.),
                        Radius::ZERO,
                        Color::from_rgb8(0, 105, 202),
                    );
                    if variant == 2 {
                        canvas.push_text_layout_highlighted(
                            &layout,
                            Point::new(4., 12.),
                            Color::from_rgba8(196, 38, 44, 180),
                            foreground,
                            &ranges,
                            None,
                        );
                    } else {
                        canvas.push_text_layout(
                            &layout,
                            Point::new(4., 12.),
                            if variant == 0 {
                                Color::from_rgba8(196, 38, 44, 180)
                            } else {
                                foreground
                            },
                        );
                    }
                    let mut renderer = NativeRenderer::with_context(
                        &context,
                        canvas.physical_width(),
                        canvas.physical_height(),
                    )?;
                    let root = RetainedNodeId::for_owner(1);
                    let mut retained = RetainedScene::new(140, 80, scale, root)?;
                    retained
                        .transaction()
                        .insert_scene(
                            RetainedParent::content(root),
                            None,
                            RetainedNodeId::for_owner(2),
                            Rc::new(canvas),
                            transform,
                        )
                        .commit()?;
                    images.push(
                        renderer
                            .render_retained_to_image_with_text(&retained, &mut fonts, &mut text)?
                            .readback()?,
                    );
                }
                let inverse = transform.inverse();
                let width = images[0].width as usize;
                for (index, actual) in images[2].pixels.iter().enumerate() {
                    let point = inverse
                        * Point::new(
                            ((index % width) as f64 + 0.5) / f64::from(scale),
                            ((index / width) as f64 + 0.5) / f64::from(scale),
                        );
                    let edge = |v: f64| (v * f64::from(scale) - 0.5).ceil() / f64::from(scale);
                    let selected = ranges.iter().any(|r| {
                        edge(r.x0) <= point.x
                            && point.x < edge(r.x1)
                            && edge(r.y0) <= point.y
                            && point.y < edge(r.y1)
                    });
                    assert_eq!(
                        *actual,
                        images[usize::from(selected)].pixels[index],
                        "scale={scale}, transform={transform:?}, foreground={foreground:?}, point={point:?}"
                    );
                }
            }
        }
    }
    context.check_validation()?;
    Ok(())
}
