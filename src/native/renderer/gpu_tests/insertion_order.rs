//! Insertions use their actual run, including through groups and independent mask branches.
use super::context;
use crate::{
    Canvas, FillRule, Mask, MaskKind, NativeRenderer, Radius, Region,
    RetainedChildBranch as Branch, RetainedLayerDescriptor as D, RetainedNodeId as Id,
    RetainedParent as Parent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect, Shape},
};
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn leaf(rect: Rect, color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(rect, Radius::ZERO, color);
    Rc::new(canvas)
}

fn mask() -> D {
    D::Mask(Mask {
        region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
        kind: MaskKind::Luminance,
    })
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn insertions_around_clip_boundaries_match_fresh_in_groups_and_mask_branches() -> Result {
    let context = context()?;
    let id = Id::for_owner;
    let full = Rect::new(0.0, 0.0, 32.0, 32.0);
    for branch in [Branch::Content, Branch::Mask] {
        for grouped in [false, true] {
            for position in 0..3 {
                let mut scene = RetainedScene::new(32, 32, 1.0, id(156_000))?;
                scene
                    .transaction()
                    .insert_layer(Parent::content(id(156_000)), None, id(156_001), mask())
                    .commit()?;
                let parent = Parent {
                    node: id(156_001),
                    branch,
                };
                let opposite = Parent {
                    node: id(156_001),
                    branch: if branch == Branch::Content {
                        Branch::Mask
                    } else {
                        Branch::Content
                    },
                };
                scene
                    .transaction()
                    .insert_scene(
                        opposite,
                        None,
                        id(156_002),
                        leaf(full, Color::WHITE),
                        Affine::IDENTITY,
                    )
                    .commit()?;
                let parent = if grouped {
                    scene
                        .transaction()
                        .insert_group(parent, None, id(156_003))
                        .commit()?;
                    Parent::content(id(156_003))
                } else {
                    parent
                };
                scene
                    .transaction()
                    .insert_layer(
                        parent,
                        None,
                        id(156_004),
                        D::ClipPath {
                            path: Rect::new(0.0, 0.0, 16.0, 32.0).to_path(0.1),
                            transform: Affine::IDENTITY,
                            rule: FillRule::NonZero,
                            tolerance: 0.1,
                        },
                    )
                    .insert_scene(
                        Parent::content(id(156_004)),
                        None,
                        id(156_005),
                        leaf(full, Color::WHITE),
                        Affine::IDENTITY,
                    )
                    .insert_group(parent, None, id(156_006))
                    .insert_scene(
                        Parent::content(id(156_006)),
                        None,
                        id(156_007),
                        leaf(
                            Rect::new(16.0, 0.0, 32.0, 32.0),
                            Color::from_rgb8(0, 255, 0),
                        ),
                        Affine::IDENTITY,
                    )
                    .commit()?;
                let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
                renderer.render_retained(&scene)?.wait()?;
                let before = match position {
                    0 => Some(id(156_004)),
                    1 => Some(id(156_006)),
                    _ => None,
                };
                scene
                    .transaction()
                    .insert_scene(
                        parent,
                        before,
                        id(156_008),
                        leaf(full, Color::BLACK),
                        Affine::IDENTITY,
                    )
                    .commit()?;
                for revised in [false, true] {
                    if revised {
                        scene
                            .transaction()
                            .replace_scene(id(156_008), leaf(full, Color::from_rgb8(0, 0, 255)))
                            .invalidate_all()
                            .commit()?;
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
                        "branch={branch:?}, grouped={grouped}, position={position}, revised={revised}"
                    );
                }
            }
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn first_insertions_into_empty_mask_branches_keep_their_own_pixels() -> Result {
    let context = context()?;
    let id = Id::for_owner;
    let full = Rect::new(0.0, 0.0, 32.0, 32.0);
    for first_mask in [false, true] {
        let mut scene = RetainedScene::new(32, 32, 1.0, id(157_000))?;
        scene
            .transaction()
            .insert_layer(Parent::content(id(157_000)), None, id(157_001), mask())
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        renderer.render_retained(&scene)?.wait()?;
        for phase in 0..3 {
            if phase < 2 {
                let branch = if (phase == 0) == first_mask {
                    Branch::Mask
                } else {
                    Branch::Content
                };
                scene
                    .transaction()
                    .insert_scene(
                        Parent {
                            node: id(157_001),
                            branch,
                        },
                        None,
                        id(157_002 + phase),
                        leaf(full, Color::WHITE),
                        Affine::IDENTITY,
                    )
                    .commit()?;
            } else {
                let mask_leaf = id(if first_mask { 157_002 } else { 157_003 });
                scene
                    .transaction()
                    .replace_scene(mask_leaf, leaf(full, Color::BLACK))
                    .commit()?;
            }
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert_eq!(
                actual.pixels, expected.pixels,
                "first_mask={first_mask}, phase={phase}"
            );
            assert!(
                actual
                    .pixels
                    .iter()
                    .all(|&pixel| pixel == if phase == 1 { 0xffff_ffff } else { 0 })
            );
        }
    }
    context.check_validation()?;
    Ok(())
}
