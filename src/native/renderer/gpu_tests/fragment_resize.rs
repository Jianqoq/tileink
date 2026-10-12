//! Fragment edits during resize must refresh unrelated filter output in the exposed area.
use super::context;
use crate::{
    Canvas, Filter, NativeRenderer, Radius, Region, RetainedLayerDescriptor as D,
    RetainedNodeId as Id, RetainedParent as Parent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn filter(rect: Rect) -> D {
    D::Filter {
        filter: Filter::Opacity(1.0),
        sample_region: Region::rect(rect, Radius::ZERO),
    }
}

fn leaf(rect: Rect, color: Color) -> Rc<Canvas> {
    let mut canvas = Canvas::new(64, 32, 1.0);
    canvas.push_rect(rect, Radius::ZERO, color);
    Rc::new(canvas)
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn fragment_edits_with_resize_refresh_unrelated_filter_pixels() -> Result {
    let context = context()?;
    let id = |value: u64| Id::for_owner(161_000 + value);
    let left = Rect::new(0.0, 0.0, 16.0, 32.0);
    let right = Rect::new(32.0, 0.0, 64.0, 32.0);
    for edit in 0..4 {
        let mut scene = RetainedScene::new(32, 32, 1.0, id(0))?;
        let mut tx = scene.transaction();
        for index in 10..18 {
            tx.insert_scene(
                Parent::content(id(0)),
                None,
                id(index),
                leaf(Rect::new(0.0, 0.0, 64.0, 32.0), Color::BLACK),
                Affine::IDENTITY,
            );
        }
        tx.insert_layer(Parent::content(id(0)), None, id(1), filter(left))
            .insert_scene(
                Parent::content(id(1)),
                None,
                id(2),
                leaf(left, Color::WHITE),
                Affine::IDENTITY,
            )
            .insert_layer(Parent::content(id(1)), None, id(3), filter(left))
            .insert_scene(
                Parent::content(id(3)),
                None,
                id(4),
                leaf(left, Color::WHITE),
                Affine::IDENTITY,
            )
            .insert_layer(Parent::content(id(0)), None, id(5), filter(right))
            .insert_scene(
                Parent::content(id(5)),
                None,
                id(6),
                leaf(Rect::new(40.0, 0.0, 60.0, 32.0), Color::WHITE),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        renderer.render_retained(&scene)?.wait()?;
        let mut tx = scene.transaction();
        tx.resize(64, 32, 1.0);
        match edit {
            0 => {
                tx.remove_subtree(id(3));
            }
            1 => {
                tx.insert_layer(Parent::content(id(1)), None, id(7), filter(left));
            }
            2 => {
                tx.remove_subtree(id(1));
            }
            3 => {
                tx.insert_layer(Parent::content(id(0)), None, id(7), filter(right))
                    .insert_scene(
                        Parent::content(id(7)),
                        None,
                        id(8),
                        leaf(
                            Rect::new(44.0, 0.0, 56.0, 32.0),
                            Color::from_rgb8(255, 0, 0),
                        ),
                        Affine::IDENTITY,
                    );
            }
            _ => unreachable!(),
        }
        tx.commit()?;
        for phase in 0..2 {
            if phase == 1 {
                scene
                    .transaction()
                    .set_transform(id(6), Affine::translate((1.0, 0.0)))
                    .commit()?;
            }
            let actual = renderer.render_retained_to_image(&scene)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 64, 32)?;
            let expected = fresh.render_retained_to_image(&scene)?.readback()?;
            assert_eq!((actual.width, actual.height), (64, 32));
            assert_eq!(actual.pixels.len(), expected.pixels.len());
            assert_eq!(
                actual
                    .pixels
                    .iter()
                    .zip(&expected.pixels)
                    .position(|(a, b)| a != b),
                None,
                "edit={edit}, phase={phase}"
            );
            assert_eq!(actual.pixels[16 * 64 + 42], u32::MAX);
            assert_eq!(
                actual.pixels[16 * 64 + 48],
                if edit == 3 { 0xff00_00ff } else { u32::MAX }
            );
        }
    }
    context.check_validation()?;
    Ok(())
}
