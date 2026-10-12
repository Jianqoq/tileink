//! Root command order and layer revisions survive incremental structural edits.
use super::context;
use crate::{
    Canvas, FillRule, Filter, Mask, MaskKind, NativeRenderer, Radius, Region,
    RetainedLayerDescriptor as D, RetainedNodeId as Id, RetainedParent as Parent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) fn leaf(color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO, color);
    Rc::new(canvas)
}

pub(super) fn filter(opacity: f32) -> D {
    D::Filter {
        filter: Filter::Opacity(opacity),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    }
}

pub(super) fn descriptor(case: usize, revised: bool) -> D {
    let full = Rect::new(0.0, 0.0, 32.0, 32.0);
    match case {
        0 => filter(if revised { 0.25 } else { 1.0 }),
        1 => D::Opacity {
            path: full.to_path(0.1),
            transform: Affine::IDENTITY,
            tolerance: 0.1,
            opacity: if revised { 0.25 } else { 1.0 },
        },
        2 if revised => D::ClipPath {
            path: Rect::new(0.0, 0.0, 16.0, 32.0).to_path(0.1),
            transform: Affine::IDENTITY,
            rule: FillRule::NonZero,
            tolerance: 0.1,
        },
        2 => filter(1.0),
        3 => D::Mask(Mask {
            region: Region::rect(full, Radius::ZERO),
            kind: if revised {
                MaskKind::Luminance
            } else {
                MaskKind::Alpha
            },
        }),
        _ => unreachable!(),
    }
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn root_reorder_with_parameter_and_kind_updates_matches_fresh_pixels() -> Result {
    let context = context()?;
    let id = Id::for_owner;
    for case in 0..4 {
        let mut scene = RetainedScene::new(32, 32, 1.0, id(158_000))?;
        let mut tx = scene.transaction();
        tx.insert_layer(
            Parent::content(id(158_000)),
            None,
            id(158_001),
            descriptor(case, false),
        );
        let content = if case == 1 {
            tx.insert_layer(Parent::content(id(158_001)), None, id(158_002), filter(1.0));
            Parent::content(id(158_002))
        } else {
            Parent::content(id(158_001))
        };
        tx.insert_scene(
            content,
            None,
            id(158_003),
            leaf(Color::from_rgb8(255, 0, 0)),
            Affine::IDENTITY,
        );
        if case == 3 {
            tx.insert_scene(
                Parent::mask(id(158_001)),
                None,
                id(158_004),
                leaf(Color::BLACK),
                Affine::IDENTITY,
            );
        }
        tx.insert_layer(Parent::content(id(158_000)), None, id(158_005), filter(1.0))
            .insert_scene(
                Parent::content(id(158_005)),
                None,
                id(158_006),
                leaf(Color::from_rgb8(0, 0, 255)),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        renderer.render_retained(&scene)?.wait()?;
        scene
            .transaction()
            .update_layer(id(158_001), descriptor(case, true))
            .move_before(id(158_005), id(158_001))
            .commit()?;
        for full in [false, true] {
            if full {
                scene.transaction().invalidate_all().commit()?;
            }
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(
                actual
                    .pixels
                    .iter()
                    .zip(&expected.pixels)
                    .position(|(a, b)| a != b),
                None,
                "case={case}, full={full}"
            );
            let left = actual.pixels[8 * 32 + 8];
            let right = actual.pixels[8 * 32 + 24];
            match case {
                0 | 1 => {
                    assert!(left & 255 > 0 && left & 255 < 200);
                    assert!(left >> 16 & 255 > 0);
                }
                2 => {
                    assert_eq!(left, 0xff00_00ff);
                    assert_eq!(right, 0xffff_0000);
                }
                3 => assert_eq!(left, 0xffff_0000),
                _ => unreachable!(),
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn middle_root_insertion_keeps_order_after_a_later_full_compilation() -> Result {
    let context = context()?;
    let id = Id::for_owner;
    let mut scene = RetainedScene::new(32, 32, 1.0, id(159_000))?;
    let mut tx = scene.transaction();
    for (layer, color) in [
        (159_001, Color::from_rgb8(255, 0, 0)),
        (159_003, Color::from_rgb8(0, 0, 255)),
    ] {
        tx.insert_layer(Parent::content(id(159_000)), None, id(layer), filter(1.0))
            .insert_scene(
                Parent::content(id(layer)),
                None,
                id(layer + 1),
                leaf(color),
                Affine::IDENTITY,
            );
    }
    tx.commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    renderer.render_retained(&scene)?.wait()?;
    scene
        .transaction()
        .insert_layer(
            Parent::content(id(159_000)),
            Some(id(159_001)),
            id(159_005),
            filter(1.0),
        )
        .insert_scene(
            Parent::content(id(159_005)),
            None,
            id(159_006),
            leaf(Color::from_rgb8(0, 255, 0)),
            Affine::IDENTITY,
        )
        .commit()?;
    for phase in 0..3 {
        if phase == 1 {
            scene
                .transaction()
                .update_layer(id(159_003), descriptor(2, true))
                .commit()?;
        } else if phase == 2 {
            scene.transaction().invalidate_all().commit()?;
        }
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
        let expected = fresh.render_retained_to_image(&scene)?.readback()?;
        assert_eq!(
            actual
                .pixels
                .iter()
                .zip(&expected.pixels)
                .position(|(a, b)| a != b),
            None,
            "phase={phase}"
        );
        assert_eq!(actual.pixels[8 * 32 + 8], 0xffff_0000);
        assert_eq!(
            actual.pixels[8 * 32 + 24],
            if phase == 0 { 0xffff_0000 } else { 0xff00_00ff }
        );
    }
    context.check_validation()?;
    Ok(())
}
