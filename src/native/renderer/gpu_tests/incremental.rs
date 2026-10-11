use super::*;
use crate::{
    ExternalTextureHistoryId, IncrementalOutputMode, NativeRenderTarget, Radius, RetainedNodeId,
    RetainedParent, RetainedScene,
};
use peniko::{
    Color,
    kurbo::{Affine, BezPath, Rect, Shape},
};

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn output_history_statistics_follow_actual_native_routes() -> Result<(), Box<dyn std::error::Error>>
{
    let context = context()?;
    let root = RetainedNodeId::for_owner(1);
    let mut scene = RetainedScene::new(32, 16, 1.0, root)?;
    let mut canvas = Canvas::new(32, 16, 1.0);
    canvas.push_rect(Rect::new(1.0, 2.0, 7.0, 14.0), Radius::ZERO, Color::WHITE);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(2),
            Rc::new(canvas),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut reference = NativeRenderer::with_context(&context, 32, 16)?;
    let expected = reference.render_retained_to_image(&scene)?.readback()?;
    let mut renderer = NativeRenderer::with_context(&context, 32, 16)?;
    for route in 0..5 {
        let origin = if route == 4 { [2, 2] } else { [0, 0] };
        let texture = context.create_texture(32 + origin[0] * 2, 16 + origin[1] * 2)?;
        for repeat in 0..2 {
            let actual = if route == 0 {
                renderer.render_retained_to_image(&scene)?.readback()?
            } else {
                let target = match route {
                    1 => (&texture).into(),
                    2 | 4 => NativeRenderTarget::persistent(
                        &texture,
                        ExternalTextureHistoryId::new(route),
                    ),
                    3 => NativeRenderTarget::transient(&texture),
                    _ => unreachable!(),
                }
                .with_origin(origin[0], origin[1]);
                renderer.render_retained_to_target(&scene, target)?.wait()?;
                texture.readback()?.readback()?
            };
            let stats = renderer.incremental_render_stats();
            let copied = route >= 3;
            assert_eq!(
                stats.output_mode,
                if matches!(route, 1 | 2) {
                    IncrementalOutputMode::ExternalHistory
                } else {
                    IncrementalOutputMode::InternalHistory
                }
            );
            assert_eq!(stats.history_copied_to_output, copied);
            if repeat == 1 {
                assert_eq!(stats.dirty_tiles, 0);
            }
            for y in 0..actual.height {
                for x in 0..actual.width {
                    let pixel = if x >= origin[0]
                        && x < origin[0] + 32
                        && y >= origin[1]
                        && y < origin[1] + 16
                    {
                        expected.pixels[((y - origin[1]) * 32 + x - origin[0]) as usize]
                    } else {
                        0
                    };
                    assert_eq!(
                        actual.pixels[(y * actual.width + x) as usize],
                        pixel,
                        "route {route}, repeat {repeat}, ({x},{y})"
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn native_path_scan_preserves_holes_and_growing_allocations()
-> Result<(), Box<dyn std::error::Error>> {
    let context = context()?;
    let leaf = |complex| {
        let mut canvas = Canvas::new(32, 32, 1.0);
        let path = if complex {
            let mut path = BezPath::new();
            path.move_to((2.0, 2.0));
            for index in 1..=600 {
                path.line_to((
                    2.0 + 26.0 * index as f64 / 600.0,
                    2.0 + (index % 2) as f64 * 2.0,
                ));
            }
            path.line_to((28.0, 30.0));
            path.line_to((2.0, 30.0));
            path.close_path();
            path
        } else {
            Rect::new(2.0, 2.0, 28.0, 30.0).to_path(0.1)
        };
        canvas.push_path(
            path,
            Color::from_rgb8(25, 150, 220),
            Affine::IDENTITY,
            crate::FillRule::NonZero,
            0.1,
        );
        Rc::new(canvas)
    };
    let root = RetainedNodeId::for_owner(1);
    let mut scene = RetainedScene::new(256, 32, 1.0, root)?;
    let simple = leaf(false);
    let mut tx = scene.transaction();
    for index in 0..8 {
        tx.insert_scene(
            RetainedParent::content(root),
            None,
            RetainedNodeId::for_owner(10 + index),
            simple.clone(),
            Affine::translate((index as f64 * 32.0, 0.0)),
        );
    }
    tx.commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 256, 32)?;
    for frame in 0..4 {
        match frame {
            1 => {
                scene
                    .transaction()
                    .remove_subtree(RetainedNodeId::for_owner(13))
                    .commit()?;
            }
            2 => {
                scene
                    .transaction()
                    .replace_scene(RetainedNodeId::for_owner(16), leaf(true))
                    .commit()?;
            }
            3 => {
                scene
                    .transaction()
                    .set_transform(
                        RetainedNodeId::for_owner(16),
                        Affine::translate((196.0, 0.0)),
                    )
                    .commit()?;
            }
            _ => {}
        }
        let actual = renderer.render_retained_to_image(&scene)?.readback()?;
        if frame == 1 {
            let paths = &renderer
                .persistent_scene
                .as_ref()
                .unwrap()
                .canvas()
                .path_records;
            assert!(
                paths
                    .iter()
                    .enumerate()
                    .any(|(index, path)| !path.is_live_at(index)),
                "exercise a real persistent path hole"
            );
        }
        let mut fresh = NativeRenderer::with_context(&context, 256, 32)?;
        assert_eq!(
            actual.pixels,
            fresh.render_retained_to_image(&scene)?.readback()?.pixels,
            "frame {frame}"
        );
    }
    Ok(())
}
