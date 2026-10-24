//! New nonplain Scene chunks must publish their actual offscreen execution structure.
use super::{context, root_order::leaf};
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

fn filtered_leaf(opacity: f32) -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_filter_layer(
        Filter::Opacity(opacity),
        Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    );
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        Radius::ZERO,
        Color::from_rgb8(255, 0, 0),
    );
    canvas.pop_layer();
    Rc::new(canvas)
}

fn insertion(initial: Option<Rc<Canvas>>) -> Result {
    let reinsert = initial.is_some();
    let context = context()?;
    {
        let id = Id::for_owner;
        let mut scene = RetainedScene::new(32, 32, 1.0, id(159_000))?;
        let mut tx = scene.transaction();
        for index in 0..8 {
            tx.insert_scene(
                Parent::content(id(159_000)),
                None,
                id(159_010 + index),
                leaf(Color::from_rgb8(0, 0, 255)),
                Affine::IDENTITY,
            );
        }
        if let Some(initial) = initial {
            tx.insert_scene(
                Parent::content(id(159_000)),
                None,
                id(159_001),
                initial,
                Affine::IDENTITY,
            );
        }
        tx.commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        renderer.render_retained(&scene)?.wait()?;
        let mut tx = scene.transaction();
        if reinsert {
            tx.remove_subtree(id(159_001));
        }
        tx.insert_scene(
            Parent::content(id(159_000)),
            None,
            id(159_001),
            filtered_leaf(0.25),
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
            assert_eq!(actual.pixels.len(), expected.pixels.len());
            assert_eq!(
                actual
                    .pixels
                    .iter()
                    .zip(&expected.pixels)
                    .position(|(a, b)| a != b),
                None,
                "reinsert={reinsert}, full={full}"
            );
            let pixel = actual.pixels[16 * 32 + 16];
            assert!((63..=65).contains(&(pixel & 255)), "pixel={pixel:08x}");
            assert!(
                (190..=192).contains(&((pixel >> 16) & 255)),
                "pixel={pixel:08x}"
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn inserting_a_scene_with_offscreen_content_matches_fresh_pixels() -> Result {
    insertion(None)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn reinserting_a_scene_with_offscreen_content_matches_fresh_pixels() -> Result {
    insertion(Some(leaf(Color::from_rgb8(255, 0, 0))))
}

fn boundary(reorder: bool) -> Result {
    let context = context()?;
    {
        let id = Id::for_owner;
        let root = Parent::content(id(159_100));
        let mut scene = RetainedScene::new(32, 32, 1.0, id(159_100))?;
        let mut tx = scene.transaction();
        tx.insert_scene(
            root,
            None,
            id(159_101),
            leaf(Color::from_rgb8(0, 0, 255)),
            Affine::IDENTITY,
        )
        .insert_scene(
            root,
            None,
            id(159_102),
            filtered_leaf(0.25),
            Affine::IDENTITY,
        );
        if reorder {
            tx.insert_scene(
                root,
                None,
                id(159_103),
                leaf(Color::from_rgb8(0, 255, 0)),
                Affine::IDENTITY,
            );
        }
        tx.commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        renderer.render_retained(&scene)?.wait()?;
        let mut tx = scene.transaction();
        if reorder {
            tx.reparent(id(159_101), root, None);
        } else {
            tx.insert_scene(
                root,
                None,
                id(159_103),
                leaf(Color::from_rgb8(0, 255, 0)),
                Affine::IDENTITY,
            );
        }
        tx.commit()?;
        for full in [false, true] {
            if full {
                scene.transaction().invalidate_all().commit()?;
            }
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(actual.pixels.len(), expected.pixels.len());
            assert_eq!(
                actual
                    .pixels
                    .iter()
                    .zip(&expected.pixels)
                    .position(|(a, b)| a != b),
                None,
                "reorder={reorder}, full={full}"
            );
            assert_eq!(
                actual.pixels[16 * 32 + 16],
                if reorder { 0xffff_0000 } else { 0xff00_ff00 }
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn appending_plain_content_after_offscreen_content_matches_fresh_pixels() -> Result {
    boundary(false)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn moving_plain_content_across_offscreen_content_matches_fresh_pixels() -> Result {
    boundary(true)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn reinserting_a_filtered_scene_retires_its_cached_pixels() -> Result {
    insertion(Some(filtered_leaf(0.75)))
}
