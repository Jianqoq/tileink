use super::context;
use crate::{
    Canvas, Filter, NativeRenderer, Radius, Region, RetainedNodeId as Id, RetainedParent as Parent,
    RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, Rect},
};
use std::rc::Rc;

fn scene(rect: Rect, domain: usize) -> Canvas {
    let mut canvas = Canvas::new(32, 32, 1.0);
    let full = Rect::new(0.0, 0.0, 32.0, 32.0);
    if domain == 1 {
        canvas.push_filter_layer(
            Filter::Offset { dx: 4.0, dy: 4.0 },
            Region::rect(full, Radius::ZERO),
        );
    }
    canvas.push_clip_sdf_rect_layer(rect, Radius::ZERO);
    if domain == 2 {
        canvas.push_filter_layer(
            Filter::Offset { dx: 4.0, dy: 4.0 },
            Region::rect(full, Radius::ZERO),
        );
    }
    canvas.push_rect(full, Radius::ZERO, Color::WHITE);
    canvas.pop_layer();
    if domain != 0 {
        canvas.pop_layer();
    }
    canvas
}

fn rectangles() -> [Rect; 5] {
    [
        Rect::new(2.0, 3.0, 16.0, 21.0),
        Rect::new(8.0, 9.0, 24.0, 30.0),
        Rect::ZERO,
        Rect::new(0.0, 0.0, 32.0, 32.0),
        Rect::new(2.0, 3.0, 16.0, 21.0),
    ]
}

fn check(pixels: &[u32], rect: Rect, domain: usize) {
    for y in 0..32 {
        for x in 0..32 {
            let point = (x as f64 + 0.5, y as f64 + 0.5);
            let inside = match domain {
                1 => rect.contains((point.0 - 4.0, point.1 - 4.0)),
                2 => rect.contains(point) && x >= 4 && y >= 4,
                _ => rect.contains(point),
            };
            assert_eq!(
                pixels[y * 32 + x],
                if inside { 0xffff_ffff } else { 0 },
                "domain={domain}, rect={rect:?}, ({x}, {y})"
            );
        }
    }
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn immediate_sdf_clip_geometry_updates_match_fresh_and_analytic_pixels()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    for domain in 0..3 {
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        for rect in rectangles() {
            let canvas = scene(rect, domain);
            let actual = renderer.render_to_image(&canvas)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
            assert_eq!(
                actual.pixels,
                fresh.render_to_image(&canvas)?.readback()?.pixels
            );
            check(&actual.pixels, rect, domain);
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_sdf_clip_geometry_updates_match_fresh_and_analytic_pixels()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    for domain in 0..3 {
        let root = Id::for_owner(152_000);
        let leaf = Id::for_owner(152_001);
        let mut retained = RetainedScene::new(32, 32, 1.0, root)?;
        retained
            .transaction()
            .insert_scene(
                Parent::content(root),
                None,
                leaf,
                Rc::new(scene(rectangles()[0], domain)),
                Affine::IDENTITY,
            )
            .commit()?;
        let mut renderer = NativeRenderer::with_context(&context, 32, 32)?;
        for rect in rectangles() {
            retained
                .transaction()
                .replace_scene(leaf, Rc::new(scene(rect, domain)))
                .commit()?;
            let actual = renderer.render_retained_to_image(&retained)?.readback()?;
            let mut fresh = NativeRenderer::with_context(&context, 32, 32)?;
            assert_eq!(
                actual.pixels,
                fresh
                    .render_retained_to_image(&retained)?
                    .readback()?
                    .pixels
            );
            check(&actual.pixels, rect, domain);
        }
    }
    Ok(())
}
