//! Hierarchy edits must retain final clip/mask scopes, independently of patch order.
use super::context;
use crate::{
    Canvas, Filter, Mask, MaskKind, NativeRenderer, Radius, Region, RetainedLayerDescriptor as D,
    RetainedNodeId as Id, RetainedParent as Parent, RetainedScene, Sdf, SdfRect,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn id(value: u64) -> Id {
    Id::for_owner(235_000 + value)
}
fn rect() -> Rect {
    Rect::new(0.0, 0.0, 32.0, 32.0)
}
fn filter() -> D {
    D::Filter {
        filter: Filter::Opacity(0.5),
        sample_region: Region::rect(rect(), Radius::ZERO),
    }
}
fn leaf(bounds: Rect, color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(bounds, Radius::ZERO, color);
    Rc::new(canvas)
}
fn scene(mask: bool) -> Result<RetainedScene> {
    let mut scene = RetainedScene::new(32, 32, 1.0, id(0))?;
    let mut tx = scene.transaction();
    tx.insert_scene(
        Parent::content(id(0)),
        None,
        id(20),
        leaf(rect(), Color::from_rgb8(0, 0, 255)),
        Affine::IDENTITY,
    );
    if mask {
        tx.insert_layer(
            Parent::content(id(0)),
            None,
            id(1),
            D::Mask(Mask {
                region: Region::rect(rect(), Radius::ZERO),
                kind: MaskKind::Alpha,
            }),
        );
        for (inner, child, parent, bounds, color) in [
            (
                3,
                4,
                Parent::content(id(1)),
                rect(),
                Color::from_rgb8(255, 0, 0),
            ),
            (
                6,
                7,
                Parent::mask(id(1)),
                Rect::new(0.0, 0.0, 16.0, 32.0),
                Color::WHITE,
            ),
        ] {
            tx.insert_layer(parent, None, id(inner), filter())
                .insert_scene(
                    Parent::content(id(inner)),
                    None,
                    id(child),
                    leaf(bounds, color),
                    Affine::IDENTITY,
                );
        }
    } else {
        tx.insert_layer(Parent::content(id(0)), None, id(12), filter());
        for (clip, parent, inner, child, x, color) in [
            (10, 1, 3, 4, 0.0, Color::from_rgb8(255, 0, 0)),
            (11, 5, 6, 7, 16.0, Color::from_rgb8(0, 255, 0)),
        ] {
            tx.insert_layer(
                Parent::content(id(12)),
                None,
                id(clip),
                D::ClipSdf {
                    sdf: Sdf::Rect(SdfRect {
                        start: (x, 0.0).into(),
                        end: (x + 16.0, 32.0).into(),
                        radius: Radius::ZERO,
                    }),
                    transform: Affine::IDENTITY,
                },
            )
            .insert_layer(Parent::content(id(clip)), None, id(parent), filter())
            .insert_layer(Parent::content(id(parent)), None, id(inner), filter())
            .insert_scene(
                Parent::content(id(inner)),
                None,
                id(child),
                leaf(rect(), color),
                Affine::IDENTITY,
            );
        }
    }
    tx.commit()?;
    Ok(scene)
}

fn compare(
    renderer: &mut NativeRenderer,
    context: &crate::NativeContext,
    scene: &RetainedScene,
    left: u32,
    right: u32,
) -> Result {
    let actual = renderer.render_retained_to_image(scene)?.readback()?;
    let mut fresh = NativeRenderer::with_context(context, 32, 32)?;
    let expected = fresh.render_retained_to_image(scene)?.readback()?;
    assert_eq!(actual.pixels, expected.pixels);
    for (x, analytic) in [(8, left), (24, right)] {
        let pixel = actual.pixels[16 * 32 + x];
        for shift in [0, 8, 16, 24] {
            assert!(
                ((pixel >> shift & 255) as i32 - (analytic >> shift & 255) as i32).abs() <= 1,
                "x={x}: pixel={pixel:08x}, analytic={analytic:08x}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn swapping_offscreen_subtrees_preserves_new_ancestor_clip_pixels() -> Result {
    let context = context()?;
    let mut scene = scene(false)?;
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    let (red, green) = (0xffdf_0020, 0xffdf_2000);
    compare(&mut renderer, &context, &scene, red, green)?;
    for iteration in 0..8 {
        let swapped = iteration % 2 == 0;
        let (a, b, left, right) = if swapped {
            (5, 1, green, red)
        } else {
            (1, 5, red, green)
        };
        scene
            .transaction()
            .reparent(id(3), Parent::content(id(a)), None)
            .reparent(id(6), Parent::content(id(b)), None)
            .commit()?;
        compare(&mut renderer, &context, &scene, left, right)?;
    }
    scene.transaction().invalidate_all().commit()?;
    compare(&mut renderer, &context, &scene, red, green)?;
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn swapping_mask_subtrees_preserves_content_and_mask_pixels() -> Result {
    let context = context()?;
    let mut scene = scene(true)?;
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    compare(&mut renderer, &context, &scene, 0xffbf_0040, 0xffff_0000)?;
    for iteration in 0..8 {
        let (a, b, left) = if iteration % 2 == 0 {
            (Parent::mask(id(1)), Parent::content(id(1)), 0xffff_4040)
        } else {
            (Parent::content(id(1)), Parent::mask(id(1)), 0xffbf_0040)
        };
        scene
            .transaction()
            .reparent(id(3), a, None)
            .reparent(id(6), b, None)
            .commit()?;
        compare(&mut renderer, &context, &scene, left, 0xffff_0000)?;
    }
    scene.transaction().invalidate_all().commit()?;
    compare(&mut renderer, &context, &scene, 0xffbf_0040, 0xffff_0000)?;
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn moving_an_ancestor_and_child_preserves_final_tree_pixels() -> Result {
    let context = context()?;
    let mut scene = scene(false)?;
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    compare(&mut renderer, &context, &scene, 0xffdf_0020, 0xffdf_2000)?;
    for iteration in 0..8 {
        let (parent, child_parent, left, right) = if iteration % 2 == 0 {
            (5, 5, 0xffff_0000, 0xffcf_1020)
        } else {
            (10, 1, 0xffdf_0020, 0xffdf_2000)
        };
        scene
            .transaction()
            .reparent(id(1), Parent::content(id(parent)), None)
            .reparent(id(3), Parent::content(id(child_parent)), None)
            .commit()?;
        compare(&mut renderer, &context, &scene, left, right)?;
    }
    scene.transaction().invalidate_all().commit()?;
    compare(&mut renderer, &context, &scene, 0xffdf_0020, 0xffdf_2000)?;
    context.check_validation()?;
    Ok(())
}
