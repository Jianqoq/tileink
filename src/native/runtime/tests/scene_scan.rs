use super::*;
use crate::{Canvas, shared::gpu_plan::PersistentPathPlans};

fn scene(x: f64, y: f64) -> Canvas {
    let mut canvas = Canvas::new(64, 64, 1.0);
    let mut path = peniko::kurbo::BezPath::new();
    path.move_to((1.0, 1.0));
    path.line_to((x, y));
    canvas.push_path(
        path,
        crate::Brush::Solid(peniko::Color::from_rgb8(255, 0, 0)),
        peniko::kurbo::Affine::IDENTITY,
        crate::FillRule::NonZero,
        0.25,
    );
    canvas
}

#[test]
fn empty_scene_records_no_geometry_dispatches() {
    let canvas = Canvas::new(17, 19, 1.0);
    let mut plans = PersistentPathPlans::default();
    let prepared = PreparedScan::new(&canvas, &mut plans);
    let mut batch = ComputeBatch::new();
    let geometry = encode_scene(&mut batch, &prepared, 65535).unwrap();
    assert!(batch.passes().is_empty());
    assert!(batch.size(geometry.backdrops).unwrap() >= 4);
    assert!(batch.size(geometry.segments).unwrap() >= size_of::<LineSegment>());
}

#[test]
fn invalid_dispatch_limits_are_rejected_even_for_empty_scenes() {
    let canvas = Canvas::new(17, 19, 1.0);
    let mut plans = PersistentPathPlans::default();
    let prepared = PreparedScan::new(&canvas, &mut plans);
    for limit in [0, 65536] {
        let mut batch = ComputeBatch::new();
        assert!(encode_scene(&mut batch, &prepared, limit).is_err());
        assert!(batch.resources().is_empty());
    }
}

#[test]
fn geometry_stages_precede_gpu_backdrop_cumsum() {
    let canvas = scene(40.0, 40.0);
    let mut plans = PersistentPathPlans::default();
    let prepared = PreparedScan::new(&canvas, &mut plans);
    assert_eq!(
        prepared.lengths.line_count, 2,
        "fill paths close their contour"
    );
    let mut batch = ComputeBatch::new();
    let result = encode_scene(&mut batch, &prepared, 65535).unwrap();
    let entries: Vec<_> = batch.passes().iter().map(|p| p.shader.entry).collect();
    assert_eq!(
        &entries[..6],
        &[
            "scan_clear",
            "scan_count",
            "scan_prefix_chunks",
            "scan_chunk_offsets",
            "scan_apply_chunk_offsets",
            "scan_emit"
        ]
    );
    assert_eq!(entries[6], "cumsum_prefix_chunks");
    assert_eq!(
        batch.size(result.paths).unwrap(),
        bytemuck::cast_slice::<_, u8>(&canvas.path_records).len()
    );
    assert_eq!(
        batch.size(result.tile_segment_ranges).unwrap(),
        prepared.lengths.backdrop_len * size_of::<TileSegmentRange>()
    );
    assert!(
        batch.outputs().is_empty(),
        "production geometry remains on the GPU"
    );
}

#[test]
fn same_count_stale_plan_is_refreshed_for_the_new_layout() {
    let first = scene(31.0, 31.0);
    let second = scene(63.0, 15.0);
    assert_eq!(first.path_records.len(), second.path_records.len());
    assert_eq!(first.backdrop_pool_capacity, second.backdrop_pool_capacity);
    let mut reused = PersistentPathPlans::default();
    let before = PreparedScan::new(&first, &mut reused)
        .plans
        .cumsum_plan()
        .clone();
    let current = PreparedScan::new(&second, &mut reused);
    let mut fresh = PersistentPathPlans::default();
    let expected = PreparedScan::new(&second, &mut fresh);
    assert_ne!(&before, current.plans.cumsum_plan());
    assert_eq!(current.plans.cumsum_plan(), expected.plans.cumsum_plan());
    assert_eq!(current.plans.scan_chunks(), expected.plans.scan_chunks());
    assert_eq!(current.plans.scan_ranges(), expected.plans.scan_ranges());
}

#[test]
fn cumsum_grid_overflow_is_reported() {
    let canvas = scene(63.0, 63.0);
    let mut plans = PersistentPathPlans::default();
    let prepared = PreparedScan::new(&canvas, &mut plans);
    let mut batch = ComputeBatch::new();
    // The scan fits one group, but cumsum requires four row chunks.
    assert!(encode_scene(&mut batch, &prepared, 1).is_err());
}
