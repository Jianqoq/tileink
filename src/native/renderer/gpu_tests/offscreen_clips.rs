//! Incremental fragment recompilation must preserve the parent's crop in pixels.
use super::*;
use crate::{
    Filter, Mask, MaskKind, Radius, Region, RetainedLayerDescriptor as D, RetainedNodeId,
    RetainedParent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_offscreen_clips_match_fresh_pixels_after_nested_insert_and_remove()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let id = RetainedNodeId::for_owner;
    let full = Rect::new(0.0, 0.0, 32.0, 32.0);
    let crop = Rect::new(8.0, 6.0, 24.0, 26.0);
    let leaf = |color| {
        let mut canvas = Canvas::new(32, 32, 1.0);
        canvas.push_rect(full, Radius::ZERO, color);
        Rc::new(canvas)
    };
    for kind in 0..3 {
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        let mut scene = RetainedScene::new(32, 32, 1.0, id(93_000))?;
        let sample_region = Region::rect(full, Radius::ZERO);
        let effect = match kind {
            0 => D::Filter {
                filter: Filter::Offset { dx: 0.0, dy: 0.0 },
                sample_region,
            },
            1 => D::Backdrop {
                filter: Filter::Opacity(1.0),
                sample_region,
            },
            _ => D::Mask(Mask {
                region: sample_region,
                kind: MaskKind::Alpha,
            }),
        };
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(id(93_000)),
                None,
                id(93_001),
                D::ClipPath {
                    path: crop.to_path(0.1),
                    transform: Affine::IDENTITY,
                    rule: crate::FillRule::NonZero,
                    tolerance: 0.1,
                },
            )
            .insert_layer(
                RetainedParent::content(id(93_001)),
                None,
                id(93_002),
                effect,
            )
            .insert_scene(
                RetainedParent::content(id(93_002)),
                None,
                id(93_003),
                leaf(Color::WHITE),
                Affine::IDENTITY,
            )
            .commit()?;
        if kind == 2 {
            scene
                .transaction()
                .insert_scene(
                    RetainedParent::mask(id(93_002)),
                    None,
                    id(93_004),
                    leaf(Color::WHITE),
                    Affine::IDENTITY,
                )
                .commit()?;
        }
        renderer.render_retained(&scene)?.wait()?;
        scene
            .transaction()
            .insert_layer(
                RetainedParent::content(id(93_002)),
                None,
                id(93_005),
                D::Isolate {
                    path: full.to_path(0.1),
                    transform: Affine::IDENTITY,
                    tolerance: 0.1,
                },
            )
            .insert_scene(
                RetainedParent::content(id(93_005)),
                None,
                id(93_006),
                leaf(Color::from_rgb8(255, 0, 0)),
                Affine::IDENTITY,
            )
            .commit()?;
        for removed in [false, true] {
            if removed {
                scene.transaction().remove_subtree(id(93_005)).commit()?;
            }
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(
                actual.pixels, expected.pixels,
                "kind={kind}, removed={removed}"
            );
            for y in 0..32 {
                for x in 0..32 {
                    let inside = crop.contains((x as f64 + 0.5, y as f64 + 0.5));
                    assert_eq!(
                        actual.pixels[y * 32 + x] >> 24,
                        if inside { 255 } else { 0 },
                        "kind={kind}, removed={removed}, ({x},{y})"
                    );
                }
            }
        }
    }
    Ok(())
}
