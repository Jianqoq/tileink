//! Recompilation retires old fragment ranges and scalar leaf classifications.
use super::context;
use crate::{
    Canvas, FillRule, Filter, NativeRenderer, Radius, Region, RetainedLayerDescriptor as D,
    RetainedNodeId as Id, RetainedParent as Parent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn filter() -> D {
    D::Filter {
        filter: Filter::Opacity(1.0),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 64.0, 32.0), Radius::ZERO),
    }
}

fn group(opacity: bool) -> D {
    let path = Rect::new(0.0, 0.0, 64.0, 32.0).to_path(0.1);
    if opacity {
        D::Opacity {
            path,
            transform: Affine::IDENTITY,
            tolerance: 0.1,
            opacity: 0.5,
        }
    } else {
        D::ClipPath {
            path,
            transform: Affine::IDENTITY,
            rule: FillRule::NonZero,
            tolerance: 0.1,
        }
    }
}

fn content(layered: bool, color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 32, 1.0);
    for (index, color) in [
        Color::from_rgb8(255, 0, 0),
        color,
        Color::from_rgb8(0, 0, 255),
    ]
    .into_iter()
    .enumerate()
    {
        let x = index as f64 * 16.0;
        let rect = Rect::new(x, 4.0, x + 16.0, 28.0);
        if layered && index == 1 {
            canvas.push_filter_layer(Filter::Opacity(0.5), Region::rect(rect, Radius::ZERO));
        }
        canvas.push_rect(rect, Radius::ZERO, color);
        if layered && index == 1 {
            canvas.pop_layer();
        }
    }
    Rc::new(canvas)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn removing_recompiled_root_layer_preserves_sibling_pixels() -> Result {
    let context = context()?;
    let id = Id::for_owner;
    let mut scene = RetainedScene::new(64, 32, 1.0, id(153_000))?;
    scene
        .transaction()
        .insert_layer(
            Parent::content(id(153_000)),
            None,
            id(153_001),
            group(false),
        )
        .insert_layer(Parent::content(id(153_001)), None, id(153_002), filter())
        .insert_layer(Parent::content(id(153_000)), None, id(153_003), filter())
        .insert_scene(
            Parent::content(id(153_003)),
            None,
            id(153_004),
            content(false, Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 32)?;
    for phase in 0..5 {
        match phase {
            1 => {
                scene
                    .transaction()
                    .update_layer(id(153_001), group(true))
                    .commit()?;
            }
            2 => {
                scene.transaction().remove_subtree(id(153_001)).commit()?;
            }
            3 => {
                scene.transaction().invalidate_all().commit()?;
            }
            4 => {
                scene
                    .transaction()
                    .replace_scene(id(153_004), content(false, Color::BLACK))
                    .commit()?;
            }
            _ => {}
        }
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
            "phase={phase}"
        );
        assert_eq!(actual.pixels[8 * 64 + 8], 0xff00_00ff);
        assert_eq!(actual.pixels[8 * 64 + 40], 0xffff_0000);
        assert_eq!(actual.pixels[0], 0);
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn multibatch_content_revisions_preserve_filter_and_sibling_pixels() -> Result {
    let context = context()?;
    let root = Id::for_owner(154_000);
    let child = Id::for_owner(154_001);
    let mut scene = RetainedScene::new(64, 32, 1.0, root)?;
    scene
        .transaction()
        .insert_scene(
            Parent::content(root),
            None,
            child,
            content(false, Color::WHITE),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 32)?;
    for phase in 0..5 {
        if phase != 0 {
            scene
                .transaction()
                .replace_scene(
                    child,
                    content(
                        phase < 4,
                        if phase % 2 == 0 {
                            Color::WHITE
                        } else {
                            Color::from_rgb8(0, 255, 0)
                        },
                    ),
                )
                .commit()?;
        }
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
            "phase={phase}"
        );
        assert_eq!(actual.pixels[8 * 64 + 8], 0xff00_00ff);
        assert_eq!(actual.pixels[8 * 64 + 40], 0xffff_0000);
        assert_ne!(actual.pixels[8 * 64 + 24] >> 24, 0);
        assert_eq!(actual.pixels[0], 0);
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn inserting_before_a_multibatch_leaf_preserves_compositing_order() -> Result {
    let context = context()?;
    let id = Id::for_owner;
    let root = id(155_000);
    let layered = id(155_001);
    let mut scene = RetainedScene::new(64, 32, 1.0, root)?;
    let mut later = Canvas::new(64, 32, 1.0);
    later.push_rect(Rect::new(48.0, 4.0, 64.0, 28.0), Radius::ZERO, Color::WHITE);
    scene
        .transaction()
        .insert_scene(
            Parent::content(root),
            None,
            layered,
            content(true, Color::from_rgb8(0, 255, 0)),
            Affine::IDENTITY,
        )
        .insert_scene(
            Parent::content(root),
            None,
            id(155_002),
            Rc::new(later),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 32)?;
    renderer.render_retained(&scene)?.wait()?;
    let mut background = Canvas::new(64, 32, 1.0);
    background.push_rect(Rect::new(0.0, 4.0, 48.0, 28.0), Radius::ZERO, Color::BLACK);
    scene
        .transaction()
        .insert_scene(
            Parent::content(root),
            Some(layered),
            id(155_003),
            Rc::new(background),
            Affine::IDENTITY,
        )
        .commit()?;
    for full in [false, true] {
        if full {
            scene.transaction().invalidate_all().commit()?;
        }
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
            "full={full}"
        );
        assert_ne!((actual.pixels[8 * 64 + 24] >> 8) & 255, 0);
    }
    context.check_validation()?;
    Ok(())
}
