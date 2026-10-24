//! Compare erased source content with an independently constructed scene, preserving all wrappers.
use super::*;
use crate::{BlurSampling, FillRule, Filter, Radius, Region};
use peniko::{
    Color, Compose, Mix,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn early_occlusion_retained_paint_and_movement_match_unculled_reference()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{Brush, RetainedNodeId as Id, RetainedParent as Parent, RetainedScene};
    let context = context()?;
    let full = Rect::new(0.0, 0.0, 96.0, 64.0);
    let mut background = Canvas::new(96, 64, 1.0);
    for i in 0..257 {
        background.push_rect(
            full,
            Radius::ZERO,
            Color::from_rgba8((i * 17) as u8, (i * 29) as u8, (i * 3) as u8, 120),
        );
    }
    let make_cover = |alpha| {
        let mut canvas = Canvas::new(96, 64, 1.0);
        canvas.push_rect(full, Radius::ZERO, Color::from_rgba8(23, 47, 93, alpha));
        Rc::new(canvas)
    };
    let mut scene = RetainedScene::new(96, 64, 1.0, Id::for_owner(1))?;
    scene
        .transaction()
        .insert_scene(
            Parent::content(Id::for_owner(1)),
            None,
            Id::for_owner(2),
            Rc::new(background.clone()),
            Affine::IDENTITY,
        )
        .insert_scene(
            Parent::content(Id::for_owner(1)),
            None,
            Id::for_owner(3),
            make_cover(255),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut retained = NativeRenderer::with_context(&context, 96, 64)?;
    let mut reference = NativeRenderer::with_context(&context, 96, 64)?;
    for (alpha, dx) in [(255, 0.0), (254, 0.0), (0, 0.0), (255, 0.0), (255, 32.0)] {
        scene
            .transaction()
            .replace_scene(Id::for_owner(3), make_cover(alpha))
            .set_transform(Id::for_owner(3), Affine::translate((dx, 0.0)))
            .commit()?;
        let actual = retained.render_retained_to_image(&scene)?.readback()?;
        let mut canvas = background.clone();
        let rect = full + peniko::kurbo::Vec2::new(dx, 0.0);
        // Constant four-corner paint preserves pixels but cannot establish either opaque proof.
        canvas.push_rect(
            rect,
            Radius::ZERO,
            Brush::four_corner(rect, [Color::from_rgba8(23, 47, 93, alpha); 4]),
        );
        let expected = reference.render_to_image(&canvas)?.readback()?;
        assert_eq!(actual.pixels, expected.pixels, "alpha={alpha}, dx={dx}");
    }
    scene
        .transaction()
        .remove_subtree(Id::for_owner(3))
        .commit()?;
    let actual = retained.render_retained_to_image(&scene)?.readback()?;
    let expected = reference.render_to_image(&background)?.readback()?;
    assert_eq!(actual.pixels, expected.pixels);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn early_occlusion_preserves_nested_clip_group_and_filter_inputs()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let full = Rect::new(0.0, 0.0, 97.0, 81.0);
    let make = |kind, hidden| {
        let mut canvas = Canvas::new(97, 81, 1.0);
        canvas.push_rect(full, Radius::ZERO, Color::from_rgb8(70, 110, 160));
        let clip = full.inset(3.375);
        canvas.push_clip_sdf_rect_layer(clip, Radius::all(9.0));
        match kind {
            0 => {
                canvas.push_clip_layer(clip.to_path(0.1), Affine::IDENTITY, FillRule::NonZero, 0.1);
            }
            1 => canvas.push_opacity_layer(full.to_path(0.1), Affine::IDENTITY, 0.1, 0.65),
            2 => canvas.push_blend_layer(
                full.to_path(0.1),
                Affine::IDENTITY,
                0.1,
                Mix::Multiply,
                Compose::SrcOver,
            ),
            _ => canvas.push_filter_layer(
                Filter::Blur {
                    std_dev_x: 3.0,
                    std_dev_y: 3.0,
                    sampling: BlurSampling::default(),
                },
                Region::rect(full, Radius::ZERO),
            ),
        }
        if hidden {
            for i in 0..257 {
                canvas.push_rect(
                    Rect::new(16.0, 16.0, 80.0, 64.0),
                    Radius::ZERO,
                    Color::from_rgba8((i * 17) as u8, (i * 29) as u8, (i * 3) as u8, 119),
                );
            }
        }
        canvas.push_rect(full, Radius::ZERO, Color::from_rgb8(230, 60, 90));
        canvas.push_rect(
            Rect::new(20.25, 24.375, 70.75, 64.125),
            Radius::all(6.0),
            Color::from_rgba8(10, 180, 90, 90),
        );
        canvas.pop_layer();
        canvas.pop_layer();
        canvas
    };
    let mut renderer = NativeRenderer::with_context(&context, 97, 81)?;
    for kind in 0..4 {
        let expected = renderer.render_to_image(&make(kind, false))?.readback()?;
        for _ in 0..2 {
            let actual = renderer.render_to_image(&make(kind, true))?.readback()?;
            assert!(actual.pixels == expected.pixels, "wrapper kind {kind}");
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn early_occlusion_keeps_sources_when_filter_localization_changes_tile_origin()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let mut renderer = NativeRenderer::with_context(&context, 96, 64)?;
    let mut canvas = Canvas::new(96, 64, 1.0);
    canvas.push_filter_layer(
        Filter::Offset { dx: 2.0, dy: 0.0 },
        Region::rect(Rect::new(30.0, 0.0, 66.0, 64.0), Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(24.0, 0.0, 72.0, 64.0),
        Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.push_rect(
        Rect::new(32.0, 0.0, 64.0, 64.0),
        Radius::ZERO,
        Color::from_rgb8(0, 0, 255),
    );
    canvas.pop_layer();
    let image = renderer.render_to_image(&canvas)?.readback()?;
    for y in 0..64 {
        for x in 32..68 {
            assert_eq!(
                image.pixels[y * 96 + x],
                if (34..66).contains(&x) {
                    0xffff_0000
                } else {
                    0xff00_00ff
                },
                "({x}, {y})"
            );
        }
    }
    context.check_validation()?;
    Ok(())
}
