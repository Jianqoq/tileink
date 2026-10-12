//! Root insertions preserve clip stacks and existing execution boundaries.
use super::{
    context,
    root_order::{descriptor, filter, leaf},
};
use crate::{
    Canvas, Filter, NativeRenderer, Radius, Region, RetainedNodeId as Id, RetainedParent as Parent,
    RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn insertion(case: usize) -> Result {
    let context = context()?;
    let id = Id::for_owner;
    {
        let mut scene = RetainedScene::new(32, 32, 1.0, id(160_000))?;
        let mut tx = scene.transaction();
        if case == 0 {
            tx.insert_group(Parent::content(id(160_000)), None, id(160_001))
                .insert_layer(
                    Parent::content(id(160_001)),
                    None,
                    id(160_002),
                    descriptor(2, true),
                )
                .insert_scene(
                    Parent::content(id(160_002)),
                    None,
                    id(160_003),
                    leaf(Color::from_rgb8(255, 0, 0)),
                    Affine::IDENTITY,
                );
        } else if case == 1 {
            let mut canvas = Canvas::new(32, 32, 1.0);
            canvas.push_rect(
                Rect::new(0.0, 0.0, 32.0, 32.0),
                Radius::ZERO,
                Color::from_rgb8(255, 0, 0),
            );
            let left = Region::rect(Rect::new(0.0, 0.0, 16.0, 32.0), Radius::ZERO);
            canvas.push_filter_layer(Filter::Opacity(0.5), left);
            canvas.push_rect(
                Rect::new(0.0, 0.0, 16.0, 32.0),
                Radius::ZERO,
                Color::from_rgb8(0, 0, 255),
            );
            canvas.pop_layer();
            tx.insert_scene(
                Parent::content(id(160_000)),
                None,
                id(160_001),
                Rc::new(canvas),
                Affine::IDENTITY,
            );
        } else {
            tx.insert_layer(Parent::content(id(160_000)), None, id(160_001), filter(1.0))
                .insert_scene(
                    Parent::content(id(160_001)),
                    None,
                    id(160_002),
                    leaf(Color::from_rgb8(255, 0, 0)),
                    Affine::IDENTITY,
                );
            let mut canvas = Canvas::new(32, 32, 1.0);
            canvas.push_rect(
                Rect::new(0.0, 0.0, 16.0, 32.0),
                Radius::ZERO,
                Color::from_rgb8(0, 0, 255),
            );
            tx.insert_scene(
                Parent::content(id(160_000)),
                None,
                id(160_003),
                Rc::new(canvas),
                Affine::IDENTITY,
            );
        }
        tx.commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        renderer.render_retained(&scene)?.wait()?;
        scene
            .transaction()
            .insert_layer(
                Parent::content(id(160_000)),
                Some(id(160_001)),
                id(160_010),
                filter(1.0),
            )
            .insert_scene(
                Parent::content(id(160_010)),
                None,
                id(160_011),
                leaf(Color::from_rgb8(0, 255, 0)),
                Affine::IDENTITY,
            )
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
        }
        // Removing a command-scope layer through a Group uses the same indexed root
        // fragment contract; removing a multibatch Scene compiles its new structure.
        scene
            .transaction()
            .remove_subtree(id(if case == 0 { 160_002 } else { 160_001 }))
            .commit()?;
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
        let expected = fresh.render_retained_to_image(&scene)?.readback()?;
        assert_eq!(actual.pixels, expected.pixels, "case={case}, after removal");
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn root_insertion_before_a_grouped_clip_preserves_its_stack() -> Result {
    insertion(0)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn root_insertion_before_a_multibatch_scene_preserves_operation_order() -> Result {
    insertion(1)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn root_insertion_does_not_move_plain_leaves_across_a_later_layer() -> Result {
    insertion(2)
}
