use super::*;
fn canvas(width: u32, height: u32) -> Canvas {
    use peniko::kurbo::Shape;
    let mut canvas = Canvas::new(width, height, 1.0);
    canvas.push_path(
        peniko::kurbo::Rect::new(1.0, 1.0, f64::from(width - 1), f64::from(height - 1))
            .to_path(0.25),
        crate::Brush::Solid(peniko::Color::from_rgb8(12, 34, 56)),
        peniko::kurbo::Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.25,
    );
    canvas
}
#[test]
fn scene_cache_reuses_execution_metadata_and_refreshes_canvas_geometry() -> Result<()> {
    let mut cache = SceneCache::default();
    let first = canvas(32, 32);
    let a = cache.record(&mut ComputeBatch::new(), &first, None, None, 65535)?;
    let b = cache.record(&mut ComputeBatch::new(), &first, None, None, 65535)?;
    assert!(Rc::ptr_eq(&a.plan, &b.plan));
    let second = canvas(64, 16);
    let mut batch = ComputeBatch::new();
    let c = cache.record(&mut batch, &second, None, None, 65535)?;
    assert_eq!(a.lengths.tile_count, c.lengths.tile_count);
    assert_eq!((a.lengths.tiles_width, a.lengths.tiles_height), (2, 2));
    assert_eq!((c.lengths.tiles_width, c.lengths.tiles_height), (4, 1));
    assert_eq!((c.fine_params.width, c.fine_params.height), (64, 16));
    assert_eq!(
        batch.resources()[c.scan.paths.index()].bytes(),
        bytemuck::cast_slice::<_, u8>(&second.path_records)
    );
    assert!(
        batch.outputs().is_empty(),
        "production assembly does not request intermediate readback"
    );
    Ok(())
}
#[test]
fn scene_rejects_foreign_batches_and_invalid_layer_ranges() -> Result<()> {
    let mut cache = SceneCache::default();
    let mut batch = ComputeBatch::new();
    let scene = cache.record(&mut batch, &canvas(32, 32), None, None, 65535)?;
    let passes = batch.passes().len();
    assert!(
        scene
            .encode_coarse(&mut batch, 0..1, 0..1, false, 65535)
            .is_err()
    );
    assert_eq!(batch.passes().len(), passes);
    let mut foreign = ComputeBatch::new();
    assert!(
        scene
            .encode_coarse(&mut foreign, 0..1, 0..0, false, 65535)
            .is_err()
    );
    assert!(foreign.passes().is_empty());
    scene.encode_coarse(&mut batch, 0..1, 0..0, false, 65535)?;
    Ok(())
}
#[test]
fn failed_scene_preparation_can_be_retried_without_stale_plan_state() -> Result<()> {
    let mut cache = SceneCache::default();
    let canvas = canvas(32, 32);
    assert!(
        cache
            .record(&mut ComputeBatch::new(), &canvas, None, None, 0)
            .is_err()
    );
    let scene = cache.record(&mut ComputeBatch::new(), &canvas, None, None, 65535)?;
    assert_eq!(scene.lengths.path_count, 1);
    Ok(())
}

#[test]
fn scene_batch_ids_are_not_physical_draw_ranges() -> Result<()> {
    let mut batch = ComputeBatch::new();
    let scene = SceneCache::default().record(&mut batch, &canvas(32, 32), None, None, 65535)?;
    // A batch without members yields zero counts; it does not index record 77.
    scene.encode_coarse(&mut batch, 77..78, 0..0, false, 65535)?;
    Ok(())
}

#[test]
fn scene_filters_reuse_scan_buffers_and_reject_foreign_batches() -> Result<()> {
    use crate::native::runtime::program::filter::stack::{Composite, Textures};
    use crate::shared::filter_config::FilterConfig;
    let mut batch = ComputeBatch::new();
    let scene = SceneCache::default().record(&mut batch, &canvas(32, 32), None, None, 65535)?;
    let geometry = scene.filter_geometry();
    let stack = scene.filter_stack();
    let target = batch.texture_rgba8([32, 32], vec![0; 32 * 32 * 4])?;
    let config = FilterConfig {
        width: 32,
        height: 32,
        region_width: 32,
        region_height: 32,
        ..Default::default()
    };
    let resources = batch.resources().len();
    geometry.mask(&mut batch, config, None, target)?;
    assert_eq!(
        batch.resources().len(),
        resources + 2,
        "only uniform and active-list placeholders are uploaded"
    );
    let pass = batch.passes().last().unwrap();
    assert!(
        pass.bindings
            .iter()
            .any(|(binding, id)| binding.slot == 24 && *id == scene.scan.segments)
    );
    assert!(batch.outputs().is_empty());
    let mut foreign = ComputeBatch::new();
    let foreign_target = foreign.texture_rgba8([32, 32], vec![0; 32 * 32 * 4])?;
    assert!(
        geometry
            .mask(&mut foreign, config, None, foreign_target)
            .is_err()
    );
    assert!(foreign.passes().is_empty());
    let passes = batch.passes().len();
    assert!(
        stack
            .encode(
                &mut batch,
                Composite::Over,
                FilterConfig {
                    layer_stack_end: 1,
                    ..config
                },
                None,
                Textures {
                    source: target,
                    auxiliary: None,
                    target
                }
            )
            .is_err()
    );
    assert_eq!(batch.passes().len(), passes);
    Ok(())
}

#[test]
fn scene_layer_bounds_follow_uploaded_records_not_replaced_metadata() -> Result<()> {
    use crate::native::runtime::program::filter::stack::{Composite, Textures};
    use crate::shared::filter_config::FilterConfig;
    use peniko::kurbo::{Affine, Rect, Shape};
    let mut batch = ComputeBatch::new();
    let mut cache = SceneCache::default();
    let mut scene = cache.record(&mut batch, &canvas(32, 32), None, None, 65535)?;
    let mut changed = Canvas::new(32, 32, 1.0);
    let path = Rect::new(1.0, 1.0, 31.0, 31.0).to_path(0.25);
    changed.push_clip_layer(
        path.clone(),
        Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.25,
    );
    changed.push_path(
        path,
        crate::Brush::Solid(peniko::Color::from_rgb8(255, 0, 0)),
        Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.25,
    );
    changed.pop_layer();
    let replacement = cache.record(&mut ComputeBatch::new(), &changed, None, None, 65535)?;
    assert_eq!(replacement.plan.layer_stack_data.len(), 1);
    // Simulate the formerly permitted metadata replacement. Actual GPU storage
    // must bound accesses even if internal metadata is changed independently.
    scene.plan = replacement.plan;
    let passes = batch.passes().len();
    assert!(
        scene
            .encode_coarse(&mut batch, 0..1, 0..1, false, 65535)
            .is_err()
    );
    assert_eq!(batch.passes().len(), passes);
    let source = batch.texture_rgba8([32, 32], vec![0; 32 * 32 * 4])?;
    let target = batch.texture_rgba8([32, 32], vec![0; 32 * 32 * 4])?;
    assert!(
        scene
            .filter_stack()
            .encode(
                &mut batch,
                Composite::Over,
                FilterConfig {
                    width: 32,
                    height: 32,
                    region_width: 32,
                    region_height: 32,
                    layer_stack_end: 1,
                    ..Default::default()
                },
                None,
                Textures {
                    source,
                    auxiliary: None,
                    target
                }
            )
            .is_err()
    );
    assert_eq!(batch.passes().len(), passes);
    Ok(())
}
