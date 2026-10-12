//! A newly installed retained fragment must render every batch and filter input.
use super::*;
use crate::{
    FillRule, Filter, Radius, Region, RetainedLayerDescriptor as D, RetainedNodeId, RetainedParent,
    RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn appended_multibatch_fragment_matches_fresh_for_local_and_exterior_filter_inputs()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let id = RetainedNodeId::for_owner;
    let full = Rect::new(0.0, 0.0, 64.0, 32.0);
    for exterior in [false, true] {
        let mut content = Canvas::new(64, 32, 1.0);
        content.push_rect(
            Rect::new(0.0, 0.0, 16.0, 16.0),
            Radius::ZERO,
            Color::from_rgb8(255, 0, 0),
        );
        let source = if exterior {
            Rect::new(-16.0, 0.0, 0.0, 16.0)
        } else {
            Rect::new(16.0, 0.0, 32.0, 16.0)
        };
        content.push_filter_layer(
            Filter::Offset {
                dx: if exterior { 32.0 } else { 0.0 },
                dy: 0.0,
            },
            Region::rect(source, Radius::ZERO),
        );
        content.push_rect(source, Radius::ZERO, Color::from_rgb8(0, 255, 0));
        content.pop_layer();
        content.push_rect(
            Rect::new(32.0, 0.0, 48.0, 16.0),
            Radius::ZERO,
            Color::from_rgb8(0, 0, 255),
        );
        let content = Rc::new(content);
        let mut scene = RetainedScene::new(64, 32, 1.0, id(95_000))?;
        let mut base = Canvas::new(64, 32, 1.0);
        base.push_rect(Rect::new(48.0, 0.0, 64.0, 16.0), Radius::ZERO, Color::WHITE);
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(id(95_000)),
                None,
                id(95_001),
                Rc::new(base),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 64, 32)?;
        renderer.render_retained(&scene)?.wait()?;
        for cycle in 0..2 {
            scene
                .transaction()
                .insert_layer(
                    RetainedParent::content(id(95_000)),
                    None,
                    id(95_002),
                    D::ClipPath {
                        path: full.to_path(0.1),
                        transform: Affine::IDENTITY,
                        rule: FillRule::NonZero,
                        tolerance: 0.1,
                    },
                )
                .insert_scene(
                    RetainedParent::content(id(95_002)),
                    None,
                    id(95_003),
                    std::rc::Rc::clone(&content),
                    Affine::IDENTITY,
                )
                .commit()?;
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 64, 32)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(
                actual
                    .pixels
                    .iter()
                    .zip(&expected.pixels)
                    .position(|(a, b)| a != b),
                None,
                "exterior={exterior}, cycle={cycle}"
            );
            for (x, color) in [
                (8, 0xff00_00ff),
                (24, 0xff00_ff00),
                (40, 0xffff_0000),
                (56, 0xffff_ffff),
            ] {
                assert_eq!(
                    actual.pixels[8 * 64 + x],
                    color,
                    "exterior={exterior}, cycle={cycle}, x={x}"
                );
            }
            scene.transaction().remove_subtree(id(95_002)).commit()?;
            let removed = renderer.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(removed.pixels[8 * 64 + 24], 0);
        }
    }
    Ok(())
}
