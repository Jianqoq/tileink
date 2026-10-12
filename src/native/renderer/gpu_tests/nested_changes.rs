//! Nested removals cannot suppress structural changes outside the rebuilt ancestor.
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

fn id(value: u64) -> Id {
    Id::for_owner(160_000 + value)
}

fn filter(opacity: f32) -> D {
    D::Filter {
        filter: Filter::Opacity(opacity),
        sample_region: Region::rect(Rect::new(0.0, 0.0, 32.0, 32.0), Radius::ZERO),
    }
}

fn leaf(rect: Rect, color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(32, 32, 1.0);
    canvas.push_rect(rect, Radius::ZERO, color);
    Rc::new(canvas)
}

fn removal(case: u8) -> Result {
    let context = context()?;
    let mut scene = RetainedScene::new(32, 32, 1.0, id(0))?;
    let left = Rect::new(0.0, 0.0, 16.0, 32.0);
    let right = Rect::new(16.0, 0.0, 32.0, 32.0);
    let red = Color::from_rgb8(255, 0, 0);
    let mut tx = scene.transaction();
    for index in 10..18 {
        tx.insert_scene(
            Parent::content(id(0)),
            None,
            id(index),
            leaf(Rect::new(0.0, 0.0, 32.0, 32.0), Color::from_rgb8(0, 0, 255)),
            Affine::IDENTITY,
        );
    }
    tx.insert_layer(Parent::content(id(0)), None, id(1), filter(0.5))
        .insert_scene(
            Parent::content(id(1)),
            None,
            id(2),
            leaf(left, red),
            Affine::IDENTITY,
        )
        .insert_layer(Parent::content(id(1)), None, id(3), filter(0.5))
        .insert_scene(
            Parent::content(id(3)),
            None,
            id(4),
            leaf(left, Color::WHITE),
            Affine::IDENTITY,
        )
        .insert_scene(
            Parent::content(id(0)),
            None,
            id(5),
            leaf(
                if case == 5 {
                    Rect::new(0.0, 0.0, 32.0, 32.0)
                } else {
                    right
                },
                Color::from_rgb8(0, 255, 0),
            ),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
    renderer.render_retained(&scene)?.wait()?;
    let mut tx = scene.transaction();
    tx.remove_subtree(id(3));
    match case {
        0 => {
            tx.insert_scene(
                Parent::content(id(0)),
                None,
                id(6),
                leaf(right, red),
                Affine::IDENTITY,
            );
        }
        1 => {
            tx.remove_subtree(id(5));
        }
        2 => {
            tx.reparent(id(2), Parent::content(id(0)), None);
        }
        3 => {
            tx.insert_layer(Parent::content(id(0)), None, id(6), filter(0.25))
                .insert_scene(
                    Parent::content(id(6)),
                    None,
                    id(7),
                    leaf(right, red),
                    Affine::IDENTITY,
                );
        }
        4 => {
            let mut canvas = Canvas::new(32, 32, 1.0);
            canvas.push_filter_layer(Filter::Opacity(0.25), Region::rect(right, Radius::ZERO));
            canvas.push_rect(right, Radius::ZERO, Color::from_rgb8(0, 255, 0));
            canvas.pop_layer();
            tx.replace_scene(id(5), Rc::new(canvas));
        }
        5 => {
            tx.reparent(id(1), Parent::content(id(0)), None);
        }
        6 => {
            tx.update_layer(
                id(1),
                D::ClipPath {
                    path: left.to_path(0.1),
                    transform: Affine::IDENTITY,
                    rule: FillRule::NonZero,
                    tolerance: 0.1,
                },
            );
        }
        7 => {
            tx.reparent(id(5), Parent::content(id(1)), None);
        }
        _ => unreachable!(),
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
            "case={case}, full={full}"
        );
        let x = if [2, 5, 6].contains(&case) { 8 } else { 24 };
        let pixel = actual.pixels[16 * 32 + x];
        let analytic: u32 = match case {
            0 | 2 | 6 => 0xff00_00ff,
            1 => 0xffff_0000,
            3 => 0xff00_bf40,
            4 => 0xffbf_4000,
            5 => 0xff00_7f80,
            7 => 0xff7f_8000,
            _ => unreachable!(),
        };
        for shift in [0, 8, 16, 24] {
            assert!(
                ((pixel >> shift & 255) as i32 - (analytic >> shift & 255) as i32).abs() <= 1,
                "case={case}, pixel={pixel:08x}, analytic={analytic:08x}"
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_root_scene_insertion_match_fresh_pixels() -> Result {
    removal(0)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_unrelated_scene_removal_match_fresh_pixels() -> Result {
    removal(1)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_reparenting_out_match_fresh_pixels() -> Result {
    removal(2)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_root_layer_insertion_match_fresh_pixels() -> Result {
    removal(3)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_root_content_structure_change_match_fresh_pixels() -> Result {
    removal(4)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_ancestor_move_match_fresh_pixels() -> Result {
    removal(5)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_ancestor_fusion_match_fresh_pixels() -> Result {
    removal(6)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn nested_removal_and_reparenting_in_match_fresh_pixels() -> Result {
    removal(7)
}
