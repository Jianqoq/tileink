//! Retained geometry reuse must depend on accepted, contiguous scene versions.
use super::*;
use crate::native::runtime::gpu_support;
use crate::retained_scene::PersistentSceneMaterializer;
use crate::{NativeRenderer, RetainedNodeId, RetainedParent, RetainedScene};
use peniko::{
    Color,
    kurbo::{Affine, BezPath},
};

#[path = "scene_scan_retained/support.rs"]
mod support;
use support::*;

#[path = "scene_scan_retained/journals.rs"]
mod journals;

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn accepted_unchanged_retained_geometry_skips_scan_and_cumsum() -> Result<()> {
    let context = gpu_support::context()?;
    let scene = retained();
    let materializer = PersistentSceneMaterializer::new(&scene);
    let canvas = materializer.canvas();
    let mut cache = SceneCache::default();
    for frame in 0..2 {
        let mut batch = gpu_support::batch(&context);
        cache.record(&mut batch, &canvas, None, None, 65535)?;
        let geometry_passes = batch
            .passes()
            .iter()
            .filter(|pass| {
                pass.shader.entry.starts_with("scan_") || pass.shader.entry.starts_with("cumsum_")
            })
            .count();
        if frame == 0 {
            assert!(geometry_passes >= 7);
        } else {
            assert_eq!(
                geometry_passes, 0,
                "accepted geometry must survive an unchanged frame"
            );
        }
        context.submit_compute(&batch)?.wait()?;
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn sparse_retained_scan_preserves_unchanged_paths_and_switches_cumsum_layouts() -> Result<()> {
    let context = gpu_support::context()?;
    let mut scene = retained();
    let root = scene.root();
    for id in 902..918 {
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(id),
                Rc::new(leaf()),
                Affine::IDENTITY,
            )
            .commit()?;
    }
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut cache = SceneCache::default();
    let mut capacity = [0; 3];
    let mut sparse_frames = 0;
    for frame in 0..12 {
        if frame > 0 {
            let mut tx = scene.transaction();
            let ids: Vec<_> = if frame == 5 || frame == 8 {
                (901..918).collect()
            } else {
                vec![901 + frame % 3]
            };
            for id in ids {
                tx.set_transform(
                    RetainedNodeId::for_owner(id),
                    Affine::translate((frame as f64 * 0.125, 0.0)),
                );
            }
            tx.commit()?;
            update(&mut materializer, &scene);
        }
        let canvas = materializer.canvas();
        let needed = [
            canvas.backdrop_pool_capacity as usize * 4,
            canvas.backdrop_pool_capacity as usize * 8,
            canvas.tile_cnt as usize * size_of::<crate::shared::line_seg::LineSegment>(),
        ];
        let grew = needed
            .iter()
            .zip(capacity)
            .any(|(&needed, old)| needed > old);
        let (actual, config) = geometry(&context, &mut cache, &canvas)?;
        let config = config.unwrap();
        if grew || frame == 5 || frame == 8 {
            assert_eq!(config[6], 0, "dense frame {frame}");
        } else {
            assert_eq!(config[6], 1, "sparse frame {frame}");
            assert_eq!(config[2], 1);
            sparse_frames += 1;
        }
        for (capacity, needed) in capacity.iter_mut().zip(needed) {
            *capacity = (*capacity).max(needed.next_power_of_two());
        }
        let (expected, _) = geometry(&context, &mut SceneCache::default(), &canvas)?;
        assert_eq!(actual, expected, "frame {frame}");
    }
    assert!(sparse_frames >= 5);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn paint_edits_reuse_paths_but_abandoned_batches_force_full_scan() -> Result<()> {
    let context = gpu_support::context()?;
    let mut scene = retained();
    let root = scene.root();
    let node = RetainedNodeId::for_owner(902);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            node,
            Rc::new(paint()),
            Affine::IDENTITY,
        )
        .commit()?;
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut cache = SceneCache::default();
    let (before, _) = geometry(&context, &mut cache, &materializer.canvas())?;
    for frame in 0..4 {
        scene
            .transaction()
            .set_transform(node, Affine::translate((f64::from(frame + 1), 0.0)))
            .commit()?;
        update(&mut materializer, &scene);
        let canvas = materializer.canvas();
        let (actual, config) = geometry(&context, &mut cache, &canvas)?;
        assert!(config.is_none());
        assert_eq!(actual, before);
    }
    let canvas = materializer.canvas();
    let mut abandoned = gpu_support::batch(&context);
    cache.record(&mut abandoned, &canvas, None, None, 65535)?;
    assert_eq!(geometry_passes(&abandoned), 0);
    drop(abandoned);
    let (actual, config) = geometry(&context, &mut cache, &canvas)?;
    assert_eq!(config.unwrap()[6], 0);
    assert_eq!(actual, before);
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn sparse_scan_cumsum_preserves_multiple_chunk_rows() -> Result<()> {
    use peniko::kurbo::{Rect, Shape};
    let context = gpu_support::context()?;
    let root = RetainedNodeId::for_owner(950);
    let mut scene = RetainedScene::new(9000, 64, 1.0, root)?;
    let mut source = Canvas::new(9000, 64, 1.0);
    source.push_path(
        Rect::new(3.0, 3.0, 8993.0, 45.0).to_path(0.1),
        Color::WHITE,
        Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.1,
    );
    for id in 951..954 {
        scene
            .transaction()
            .insert_scene(
                RetainedParent::content(root),
                None,
                RetainedNodeId::for_owner(id),
                Rc::new(source.clone()),
                Affine::IDENTITY,
            )
            .commit()?;
    }
    let mut materializer = PersistentSceneMaterializer::new(&scene);
    let mut cache = SceneCache::default();
    for frame in 0..6 {
        if frame > 0 {
            scene
                .transaction()
                .set_transform(
                    RetainedNodeId::for_owner(951),
                    Affine::translate((f64::from(frame) * 0.125, 0.0)),
                )
                .commit()?;
            update(&mut materializer, &scene);
        }
        let canvas = materializer.canvas();
        let (actual, config) = geometry(&context, &mut cache, &canvas)?;
        if frame >= 3 {
            assert_eq!(config.unwrap()[6], 1);
        }
        let (expected, _) = geometry(&context, &mut SceneCache::default(), &canvas)?;
        assert_eq!(actual, expected, "long row frame {frame}");
    }
    context.check_validation()?;
    Ok(())
}

#[test]
#[ignore = "requires explicitly pinned physical GPU; run with --ignored"]
fn retained_scan_pixels_match_full_replay_after_growth_removal_and_slot_reuse() -> Result<()> {
    let context = gpu_support::context()?;
    let mut scene = retained();
    let root = scene.root();
    let path = RetainedNodeId::for_owner(901);
    let other = RetainedNodeId::for_owner(902);
    scene
        .transaction()
        .insert_scene(
            RetainedParent::content(root),
            None,
            other,
            Rc::new(leaf()),
            Affine::translate((9.0, 0.0)),
        )
        .commit()?;
    let mut renderer = NativeRenderer::with_context(&context, 96, 96)?;
    let mut images = Vec::new();
    for frame in 0..18 {
        let mut tx = scene.transaction();
        match frame {
            4 => {
                tx.replace_scene(path, Rc::new(Canvas::new(96, 96, 1.0)));
            }
            5 => {
                tx.replace_scene(path, Rc::new(leaf()));
            }
            8 => {
                tx.remove_subtree(other);
            }
            9 => {
                tx.insert_scene(
                    RetainedParent::content(root),
                    None,
                    other,
                    Rc::new(leaf()),
                    Affine::translate((9.0, 0.0)),
                );
            }
            _ => {}
        }
        tx.set_transform(
            path,
            Affine::translate((f64::from(frame) * 0.25, 0.0))
                * Affine::scale(1.0 + f64::from(frame) * 0.0125),
        )
        .commit()?;
        let actual = renderer
            .render_retained_to_image(&scene)?
            .readback()?
            .pixels;
        let mut fresh = NativeRenderer::with_context(&context, 96, 96)?;
        let expected = fresh.render_retained_to_image(&scene)?.readback()?.pixels;
        assert_eq!(actual, expected, "retained path pixels frame {frame}");
        images.push(actual);
    }
    assert_ne!(
        images[0], images[17],
        "the workload must change visible geometry"
    );
    context.check_validation()?;
    Ok(())
}
