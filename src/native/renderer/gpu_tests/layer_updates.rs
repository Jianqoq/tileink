//! Group descriptor updates must reach the same execution path as a fresh frame.
use super::*;
use crate::{
    Filter, Radius, Region, RetainedLayerDescriptor, RetainedNodeId, RetainedParent, RetainedScene,
};
use peniko::{
    Color, Compose, Mix,
    kurbo::{Affine, Rect, Shape},
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_offscreen_group_updates_match_fresh_pixels() -> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let root = RetainedNodeId::for_owner(1);
    let group = RetainedNodeId::for_owner(2);
    let filter = RetainedNodeId::for_owner(3);
    for blend in [false, true] {
        let descriptor = |value| {
            if blend {
                RetainedLayerDescriptor::Blend {
                    path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
                    transform: Affine::IDENTITY,
                    tolerance: 0.1,
                    mix: [Mix::Multiply, Mix::Screen, Mix::Overlay][value],
                    compose: Compose::SrcOver,
                }
            } else {
                RetainedLayerDescriptor::Opacity {
                    path: Rect::new(0.0, 0.0, 64.0, 64.0).to_path(0.1),
                    transform: Affine::IDENTITY,
                    tolerance: 0.1,
                    opacity: [0.25, 0.75, 1.0][value],
                }
            }
        };
        let mut background = Canvas::new(64, 64, 1.0);
        background.push_rect(
            Rect::new(0.0, 0.0, 64.0, 64.0),
            Radius::ZERO,
            Color::from_rgb8(40, 100, 180),
        );
        let mut content = Canvas::new(64, 64, 1.0);
        content.push_rect(
            Rect::new(8.0, 8.0, 40.0, 48.0),
            Radius::ZERO,
            Color::from_rgb8(220, 60, 30),
        );
        content.push_rect(
            Rect::new(24.0, 16.0, 56.0, 56.0),
            Radius::ZERO,
            Color::from_rgb8(180, 220, 60),
        );
        let mut scene = RetainedScene::new(64, 64, 1.0, root)?;
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(4),
                Rc::new(background),
                Affine::IDENTITY,
            )
            .insert_layer(RetainedParent::content(root), None, group, descriptor(0))
            .insert_layer(
                RetainedParent::content(group),
                None,
                filter,
                RetainedLayerDescriptor::Filter {
                    filter: Filter::Opacity(1.0),
                    sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 64.0), Radius::ZERO),
                },
            )
            .insert_scene(
                RetainedParent::content(filter),
                None,
                RetainedNodeId::for_owner(5),
                Rc::new(content),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 64, 64)?;
        let mut previous = Vec::new();
        for value in 0..3 {
            if value != 0 {
                scene
                    .transaction()
                    .update_layer(group, descriptor(value))
                    .commit()?;
            }
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 64, 64)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert!(
                actual.pixels == expected.pixels,
                "blend={blend}, value={value}"
            );
            assert!(
                actual.pixels != previous,
                "group parameter must change visible pixels"
            );
            previous = actual.pixels;
            renderer.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(renderer.incremental_render_stats().dirty_tiles, 0);
        }
    }
    context.check_validation()?;
    Ok(())
}
