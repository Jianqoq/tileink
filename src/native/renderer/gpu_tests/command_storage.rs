//! Recycled command IDs must preserve pixels across clips, both mask branches and kind changes.
use super::context;
use crate::{
    Canvas, Filter, Mask, MaskKind, NativeRenderer, Radius, Region, RetainedLayerDescriptor as D,
    RetainedNodeId as Id, RetainedParent as Parent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

const fn id(value: u64) -> Id {
    Id::for_owner(225_000 + value)
}
const fn rect() -> Rect {
    Rect::new(0.0, 0.0, 32.0, 32.0)
}
const fn mask() -> Mask {
    Mask {
        region: Region::rect(rect(), Radius::ZERO),
        kind: MaskKind::Alpha,
    }
}
const fn filter() -> D {
    D::Filter {
        filter: Filter::Opacity(0.5),
        sample_region: Region::rect(rect(), Radius::ZERO),
    }
}
fn leaf(bounds: Rect, color: Color) -> Canvas {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(bounds, Radius::ZERO, color);
    canvas
}

fn compare(
    renderer: &mut NativeRenderer,
    context: &crate::NativeContext,
    scene: &RetainedScene,
) -> Result {
    let actual = renderer.render_retained_to_image(scene)?.readback()?;
    let mut fresh = NativeRenderer::with_context(context, 32, 32)?;
    let expected = fresh.render_retained_to_image(scene)?.readback()?;
    assert_eq!(actual.pixels, expected.pixels);
    assert_eq!(
        actual.pixels[16 * 32 + 28],
        0xffff_0000,
        "the unchanged background remains blue"
    );
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn recycled_command_lists_preserve_both_mask_branches_and_internal_layers() -> Result {
    let context = context()?;
    for branch in [
        crate::RetainedChildBranch::Content,
        crate::RetainedChildBranch::Mask,
    ] {
        let mut scene = RetainedScene::new(32, 32, 1.0, id(0))?;
        let mut tx = scene.transaction();
        for index in 10..18 {
            tx.insert_scene(
                Parent::content(id(0)),
                None,
                id(index),
                Rc::new(leaf(rect(), Color::from_rgb8(0, 0, 255))),
                Affine::IDENTITY,
            );
        }
        tx.insert_layer(
            Parent::content(id(0)),
            None,
            id(1),
            D::ClipSdf {
                sdf: crate::Sdf::Rect(crate::SdfRect {
                    start: (0.0, 0.0).into(),
                    end: (24.0, 32.0).into(),
                    radius: Radius::ZERO,
                }),
                transform: Affine::IDENTITY,
            },
        )
        .insert_layer(Parent::content(id(1)), None, id(2), D::Mask(mask()))
        .insert_scene(
            Parent::content(id(2)),
            None,
            id(20),
            Rc::new(leaf(rect(), Color::from_rgb8(255, 0, 0))),
            Affine::IDENTITY,
        )
        .insert_scene(
            Parent::mask(id(2)),
            None,
            id(21),
            Rc::new(leaf(Rect::new(0.0, 0.0, 16.0, 32.0), Color::WHITE)),
            Affine::IDENTITY,
        )
        .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        compare(&mut renderer, &context, &scene)?;
        for _ in 0..16 {
            let mut source = Canvas::new(32, 32, 1.0);
            source.push_mask_layer(leaf(Rect::new(8.0, 0.0, 24.0, 32.0), Color::WHITE), mask());
            source.push_clip_sdf_rect_layer(rect(), Radius::ZERO);
            source.push_filter_layer(Filter::Opacity(0.75), Region::rect(rect(), Radius::ZERO));
            source.push_rect(
                rect(),
                Radius::ZERO,
                if branch == crate::RetainedChildBranch::Content {
                    Color::from_rgb8(0, 255, 0)
                } else {
                    Color::WHITE
                },
            );
            for _ in 0..3 {
                source.pop_layer();
            }
            scene
                .transaction()
                .insert_layer(
                    Parent {
                        node: id(2),
                        branch,
                    },
                    None,
                    id(3),
                    filter(),
                )
                .insert_scene(
                    Parent::content(id(3)),
                    None,
                    id(4),
                    Rc::new(source),
                    Affine::IDENTITY,
                )
                .commit()?;
            compare(&mut renderer, &context, &scene)?;
            scene.transaction().remove_subtree(id(3)).commit()?;
            compare(&mut renderer, &context, &scene)?;
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn recycled_mask_branch_remains_correct_after_kind_changes() -> Result {
    let context = context()?;
    let mut scene = RetainedScene::new(32, 32, 1.0, id(0))?;
    scene
        .transaction()
        .insert_scene(
            Parent::content(id(0)),
            None,
            id(10),
            Rc::new(leaf(rect(), Color::from_rgb8(0, 0, 255))),
            Affine::IDENTITY,
        )
        .insert_layer(Parent::content(id(0)), None, id(1), filter())
        .insert_scene(
            Parent::content(id(1)),
            None,
            id(20),
            Rc::new(leaf(
                Rect::new(0.0, 0.0, 24.0, 32.0),
                Color::from_rgb8(255, 0, 0),
            )),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    compare(&mut renderer, &context, &scene)?;
    for _ in 0..16 {
        scene
            .transaction()
            .update_layer(id(1), D::Mask(mask()))
            .commit()?;
        compare(&mut renderer, &context, &scene)?;
        scene.transaction().update_layer(id(1), filter()).commit()?;
        compare(&mut renderer, &context, &scene)?;
    }
    context.check_validation()?;
    Ok(())
}
