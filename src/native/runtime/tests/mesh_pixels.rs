use super::*;
use crate::{
    Canvas, Filter, NativeRenderer, Radius, Region, RetainedNodeId, RetainedParent, RetainedScene,
};
use peniko::kurbo::{Affine, Point, Rect};
use std::rc::Rc;

fn mesh(colors: [Color; 4]) -> MeshGradient {
    MeshGradient::new(
        2,
        2,
        [[0.0, 0.0], [32.0, 0.0], [0.0, 32.0], [32.0, 32.0]],
        colors,
    )
    .unwrap()
}
const fn colors() -> [Color; 4] {
    [
        Color::from_rgba8(255, 0, 0, 255),
        Color::from_rgba8(0, 255, 0, 128),
        Color::from_rgba8(0, 0, 255, 254),
        Color::TRANSPARENT,
    ]
}
fn canvas(scale: f32) -> Canvas {
    let mut canvas = Canvas::new(48, 48, scale);
    canvas.push_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        Radius::ZERO,
        mesh(colors()),
    );
    canvas
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_mesh_pixels_match_oracle_under_scale_append_and_affine_transform() -> Result<()> {
    let context = context()?;
    for scale in [1.0, 2.0] {
        for translated in [false, true] {
            let child = canvas(scale);
            let mut scene = Canvas::new(48, 48, scale);
            let offset = if translated { 7.0 } else { 0.0 };
            scene.append(&child, Point::new(offset, offset));
            let size = scene.physical_size();
            let mut renderer = NativeRenderer::with_context(&context, size.0, size.1)?;
            let image = renderer.render_to_image(&scene)?.readback()?;
            for y in 0..size.1 {
                for x in 0..size.0 {
                    let px = (f64::from(x) + 0.5) / f64::from(scale) - offset;
                    let py = (f64::from(y) + 0.5) / f64::from(scale) - offset;
                    let expected = if (0.0..32.0).contains(&px) && (0.0..32.0).contains(&py) {
                        interpolate(colors(), px / 32.0, py / 32.0, MeshInterpolation::Smooth)
                    } else {
                        0
                    };
                    assert_close(
                        image.pixels[(y * size.0 + x) as usize],
                        expected,
                        "scaled append",
                    );
                }
            }
        }
    }
    let child = canvas(1.0);
    let mut scene = Canvas::new(48, 48, 1.0);
    let transform = Affine::translate((40.0, 3.0)) * Affine::rotate(std::f64::consts::FRAC_PI_2);
    scene.append_transformed(&child, transform);
    let mut renderer = NativeRenderer::with_context(&context, 48, 48)?;
    let image = renderer.render_to_image(&scene)?.readback()?;
    for y in 4..34 {
        for x in 9..39 {
            let point = transform.inverse() * Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
            assert_close(
                image.pixels[(y * 48 + x) as usize],
                interpolate(
                    colors(),
                    point.x / 32.0,
                    point.y / 32.0,
                    MeshInterpolation::Smooth,
                ),
                "affine",
            );
        }
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_mesh_filter_brush_and_translated_offscreen_match_direct_pixels() -> Result<()> {
    let context = context()?;
    let bounds = Rect::new(7.0, 9.0, 39.0, 41.0);
    let points = [[7.0, 9.0], [39.0, 9.0], [7.0, 41.0], [39.0, 41.0]];
    let brush = MeshGradient::new(2, 2, points, colors())?;
    let mut direct = Canvas::new(48, 48, 1.0);
    direct.push_rect(bounds, Radius::ZERO, brush.clone());
    let mut offscreen = Canvas::new(48, 48, 1.0);
    offscreen.push_filter_layer(
        Filter::Offset { dx: 0.0, dy: 0.0 },
        Region::rect(bounds, Radius::ZERO),
    );
    offscreen.push_rect(bounds, Radius::ZERO, brush.clone());
    offscreen.pop_layer();
    let mut flood = Canvas::new(48, 48, 1.0);
    flood.push_filter_layer(
        Filter::Flood {
            brush: brush.into(),
        },
        Region::rect(bounds, Radius::ZERO),
    );
    flood.push_rect(bounds, Radius::ZERO, Color::WHITE);
    flood.pop_layer();
    let mut renderer = NativeRenderer::with_context(&context, 48, 48)?;
    let expected = renderer.render_to_image(&direct)?.readback()?;
    for scene in [&offscreen, &flood] {
        let image = renderer.render_to_image(scene)?.readback()?;
        assert_eq!(
            image.pixels, expected.pixels,
            "offscreen/filter mesh coordinates"
        );
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_mesh_retained_color_geometry_changes_and_removal_match_fresh_frames() -> Result<()> {
    let context = context()?;
    let root = RetainedNodeId::for_owner(100);
    let node = RetainedNodeId::for_owner(101);
    let mut scene = RetainedScene::new(64, 64, 1.0, root)?;
    let mut renderer = NativeRenderer::with_context(&context, 64, 64)?;
    for phase in 0..4 {
        let mut child = Canvas::new(48, 48, 1.0);
        let count = if phase < 2 { 2 } else { 3 };
        let positions: Vec<_> = (0..count)
            .flat_map(|y| (0..count).map(move |x| [x as f32 * 16.0, y as f32 * 16.0]))
            .collect();
        let colors: Vec<_> = (0..count * count)
            .map(|i| {
                Color::from_rgba8(
                    (i * 61 + phase * 30) as u8,
                    80,
                    190,
                    if phase == 1 { 128 } else { 255 },
                )
            })
            .collect();
        child.push_rect(
            Rect::new(0.0, 0.0, 32.0, 32.0),
            Radius::all(4.0),
            MeshGradient::new(count, count, positions, colors)?,
        );
        let mut tx = scene.transaction();
        if phase == 0 {
            tx.insert_scene(
                RetainedParent::content(root),
                None,
                node,
                Rc::new(child),
                Affine::IDENTITY,
            );
        } else {
            tx.replace_scene(node, Rc::new(child));
        }
        tx.set_transform(
            node,
            Affine::translate((phase as f64 * 3.0, phase as f64 * 2.0)),
        )
        .commit()?;
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        let mut fresh = NativeRenderer::with_context(&context, 64, 64)?;
        assert_eq!(
            actual.pixels,
            fresh.render_retained_to_image(&scene)?.readback()?.pixels
        );
    }
    scene.transaction().remove_subtree(node).commit()?;
    assert!(
        renderer
            .render_retained_to_image(&scene)?
            .readback()?
            .pixels
            .iter()
            .all(|&p| p == 0)
    );
    context.check_validation()?;
    Ok(())
}
