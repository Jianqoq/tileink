//! A fine-only oracle and retained exposure checks guard raster-prefix semantics.
use super::*;
use crate::{
    Canvas, NativeBackend, NativeContext, NativeContextOptions, NativeRenderer, Radius,
    RetainedNodeId, RetainedParent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;

fn context() -> Result<NativeContext> {
    #[cfg(feature = "dx12")]
    unsafe {
        NativeContext::enable_dx12_validation()?;
    }
    let backend = if cfg!(feature = "metal") {
        NativeBackend::Metal
    } else if cfg!(feature = "dx12") {
        NativeBackend::Dx12
    } else {
        NativeBackend::Vulkan
    };
    Ok(NativeContext::new(
        backend,
        &NativeContextOptions {
            validation: true,
            physical_adapter: Some(std::env::var("TILEINK_NATIVE_GPU")?),
        },
    )?)
}

fn background() -> Canvas {
    let mut c = Canvas::new(129, 113, 1.0);
    for i in 0..40 {
        c.push_rect(
            Rect::new(0., 0., 129., 113.),
            Radius::all((i % 8) as f32),
            Color::from_rgba8((i * 37) as u8, (i * 71) as u8, (i * 97) as u8, 119),
        );
    }
    c
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_matches_independent_fine_only_pixels() -> Result<()> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 129, 113)?;
    renderer.set_clear_color(Color::BLACK);
    for alpha in [0, 254, 255] {
        for radius in [0., 19.] {
            for inset in [0., 0.375] {
                let mut c = background();
                c.push_rect(
                    Rect::new(16., 0., 129., 113.).inset(inset),
                    Radius::all(radius),
                    Color::from_rgba8(230, 70, 35, alpha),
                );
                c.push_rect(
                    Rect::new(0., 32.25, 90., 79.75),
                    Radius::all(3.),
                    Color::from_rgba8(20, 130, 240, 130),
                );
                let expected = super::rect_pixels::interpret_rectangles(&context, &c)?;
                let actual = renderer.render_to_image(&c)?.readback()?;
                assert_eq!(
                    bytemuck::cast_slice::<u32, u8>(&actual.pixels),
                    expected,
                    "alpha={alpha} radius={radius} inset={inset}"
                );
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_reveals_retained_content_after_cover_changes() -> Result<()> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 129, 113)?;
    let root = RetainedNodeId::for_owner(1);
    let back = RetainedNodeId::for_owner(2);
    let cover = RetainedNodeId::for_owner(3);
    let mut scene = RetainedScene::new(129, 113, 1., root)?;
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            back,
            Rc::new(background()),
            Affine::IDENTITY,
        )
        .commit()?;
    for (index, alpha) in [255, 254, 0, 255].into_iter().enumerate() {
        let mut c = Canvas::new(129, 113, 1.);
        // Clip batches reuse preallocated ranges; their physical origins must survive.
        c.push_clip_sdf_rect_layer(Rect::new(0., 0., 129., 113.), Radius::all(13.));
        c.push_rect(
            Rect::new(0., 0., 129., 113.),
            Radius::ZERO,
            Color::from_rgba8(40, 160, 90, alpha),
        );
        c.pop_layer();
        let mut tx = scene.transaction();
        if index == 0 {
            tx.insert_scene(
                RetainedParent::content(root),
                None,
                cover,
                Rc::new(c),
                Affine::IDENTITY,
            );
        } else {
            tx.replace_scene(cover, Rc::new(c));
        }
        tx.set_transform(cover, Affine::translate((index as f64 * 9.25, 0.)))
            .commit()?;
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, 129, 113)?;
        let expected = fresh.render_retained_to_image(&scene)?.readback()?;
        assert_eq!(
            actual.pixels, expected.pixels,
            "retained cover update {index}"
        );
    }
    scene.transaction().remove_subtree(cover).commit()?;
    let actual = renderer.render_retained_to_image(&scene)?.readback()?;
    let expected = renderer.render_to_image(&background())?.readback()?;
    assert_eq!(
        actual.pixels, expected.pixels,
        "removing cover must reveal all hidden content"
    );
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_opaque_occlusion_preserves_text_and_image_destinations_after_the_cover() -> Result<()> {
    use crate::{Brush, Image, PatternSampling, TextContext, TextFontSystem, TextLayoutOptions};
    use peniko::{Extend, kurbo::Point};
    let context = context()?;
    let mut fonts = TextFontSystem::new();
    let mut text = TextContext::new();
    let layout = text.layout(
        &mut fonts,
        TextLayoutOptions::new("Visible glyphs 012345", 16.),
    );
    assert!(!layout.is_empty());
    let image = Rc::new(Image::new(8, 8, Color::from_rgba8(30, 120, 230, 230)));
    let mut outputs = Vec::new();
    for cull in [false, true] {
        let mut c = background();
        c.push_text_layout(&layout, Point::new(2., 2.), Color::WHITE);
        c.push_image(
            Rect::new(0., 32., 80., 96.),
            image.clone(),
            Extend::Pad,
            PatternSampling::Bilinear,
        );
        let full = Rect::new(0., 0., 129., 113.);
        let color = Color::from_rgb8(20, 45, 90);
        // Constant four-corner paint produces the same pixels while forcing fine
        // brush evaluation instead of the PTCL_COLOR coverage/opacity proof.
        let brush = if cull {
            Brush::Solid(color)
        } else {
            Brush::four_corner(full, [color; 4])
        };
        c.push_rect(full, Radius::ZERO, brush);
        c.push_text_layout(&layout, Point::new(8., 32.), Color::WHITE);
        c.push_image(
            Rect::new(48., 72., 113., 104.),
            image.clone(),
            Extend::Pad,
            PatternSampling::Bilinear,
        );
        let mut renderer = NativeRenderer::with_context(&context, 129, 113)?;
        outputs.push(
            renderer
                .render_to_image_with_text(&c, &mut fonts, &mut text)?
                .readback()?
                .pixels,
        );
    }
    assert_eq!(outputs[0], outputs[1]);
    context.check_validation()?;
    Ok(())
}
